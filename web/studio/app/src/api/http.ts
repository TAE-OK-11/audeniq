// 실제 API 호출 계층 — Cloudflare Worker(엣지)를 거쳐 Workers VPC → 메인 서버로 전달된다.
// 브라우저는 세션 쿠키(HttpOnly)와 CSRF 토큰만 다루고, 서비스 비밀값은 엣지만 가진다.
import { ApiError, messageForCode } from './errors';

const API_BASE = (import.meta.env.VITE_API_BASE ?? '').replace(/\/$/, '');

let csrfToken = '';

// 엣지 Worker가 직접 돌려주는 오류(본문이 JSON이 아님)를 상태 코드로 분류
const STATUS_CODES: Record<number, string> = {
  401: 'UNAUTHENTICATED', 403: 'FORBIDDEN', 404: 'NOT_FOUND', 413: 'PAYLOAD_TOO_LARGE',
  429: 'RATE_LIMITED', 502: 'DATABASE_UNAVAILABLE', 503: 'DATABASE_UNAVAILABLE', 504: 'DATABASE_UNAVAILABLE',
};
let currentOrgId = '';

export function setCsrf(token: string) { csrfToken = token; }
export function hasCsrf() { return !!csrfToken; }
export function setOrgId(id: string) { currentOrgId = id; }
import { currentPath, toHref } from '../lib/router';
import { classifyServerIssue, reportServerIssue } from '../lib/systemEvents';
export function orgId() { return currentOrgId; }

export function orgPath(path: string): string {
  if (!currentOrgId) throw new ApiError('작업 공간을 찾을 수 없어요. 다시 로그인해 주세요.', 400, 'NO_ORG');
  return `/api/orgs/${encodeURIComponent(currentOrgId)}${path}`;
}

interface ReqOptions {
  method?: 'GET' | 'POST' | 'PUT' | 'DELETE';
  body?: unknown;
  timeoutMs?: number;
  /** 401이어도 로그인 화면으로 보내지 않음 (세션 확인·로그인 요청용) */
  quiet401?: boolean;
  /** 서버 장애여도 전역 오류 창을 띄우지 않음 (화면에서 직접 안내하는 요청용) */
  quietServer?: boolean;
  signal?: AbortSignal;
}

/**
 * JSON 요청. 상태 변경 요청은 CSRF 토큰을 붙이고, 본문이 없으면 `{}`를 보낸다
 * (백엔드는 변경 요청에 JSON 본문을 요구한다).
 */
export async function req<T>(path: string, opts: ReqOptions = {}): Promise<T> {
  const method = opts.method ?? 'GET';
  const headers: Record<string, string> = { Accept: 'application/json' };
  let body: string | undefined;
  if (method !== 'GET') {
    headers['Content-Type'] = 'application/json';
    body = JSON.stringify(opts.body ?? {});
    if (csrfToken) headers['X-CSRF-Token'] = csrfToken;
  }
  const ctrl = new AbortController();
  const timer = window.setTimeout(() => ctrl.abort(), opts.timeoutMs ?? 20000);
  opts.signal?.addEventListener('abort', () => ctrl.abort());
  let res: Response;
  try {
    res = await fetch(`${API_BASE}${path}`, { method, headers, body, credentials: 'include', signal: ctrl.signal, cache: 'no-store' });
  } catch (e) {
    const aborted = e instanceof DOMException && e.name === 'AbortError';
    // 기기가 온라인인데 연결 자체가 안 되면 서버 쪽 문제로 본다 (오프라인은 SystemStatus가 따로 알린다)
    if (!aborted && !opts.quietServer && navigator.onLine && !opts.signal?.aborted) reportServerIssue({ kind: 'down', status: 0, code: 'NETWORK' });
    throw new ApiError(aborted ? '서버 응답이 늦어요. 잠시 후 다시 시도해 주세요.' : '네트워크 연결을 확인해 주세요.', 0, aborted ? 'TIMEOUT' : 'NETWORK');
  } finally {
    window.clearTimeout(timer);
  }

  const text = await res.text();
  let data: unknown = null;
  if (text) {
    try { data = JSON.parse(text); } catch { data = null; }
  }

  if (!res.ok) {
    const code = (data as { error?: { code?: string } } | null)?.error?.code
      ?? STATUS_CODES[res.status] ?? '';
    if (res.status === 401 && !opts.quiet401) {
      csrfToken = '';
      const onAuthPage = /^\/(login|signup|find-account)/.test(currentPath());
      if (!onAuthPage) window.location.assign(toHref('/login'));
    }
    const issue = classifyServerIssue(res.status, code);
    if (issue && !opts.quietServer) reportServerIssue(issue);
    const msg = code === 'PAYLOAD_TOO_LARGE' ? '보낼 정보가 너무 커요. 앨범 소개나 가사 길이를 줄여 주세요.' : messageForCode(code, res.status);
    throw new ApiError(msg, res.status, code);
  }
  return (data ?? {}) as T;
}

/** 새로고침 후 세션 쿠키로 CSRF 토큰을 다시 받는다 */
export async function bootstrapCsrf(): Promise<void> {
  const r = await req<{ csrf_token: string }>('/api/auth/csrf', { method: 'POST', quiet401: true });
  csrfToken = r.csrf_token;
}

/** 커서 페이지 목록을 끝까지 모은다 (최대 pages 페이지) */
export async function listAll<T>(path: string, pages = 20): Promise<T[]> {
  const out: T[] = [];
  let after: string | null = null;
  for (let i = 0; i < pages; i++) {
    const q = new URLSearchParams({ limit: '100' });
    if (after) q.set('after', after);
    const r: { items: T[]; next_cursor: string | null } = await req(`${path}?${q}`);
    out.push(...r.items);
    if (!r.next_cursor) break;
    after = r.next_cursor;
  }
  return out;
}

export interface UploadGrant {
  url: string;
  method: string;
  headers: Record<string, string>;
  expires_at: string;
}

/**
 * 서명된 URL로 R2에 파일을 직접 올린다 (음원 원본은 엣지 Worker를 거치지 않음).
 * 진행률을 받기 위해 XHR을 쓴다.
 */
export function putToGrant(grant: UploadGrant, file: Blob, onProgress?: (ratio: number) => void, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    xhr.open(grant.method || 'PUT', grant.url);
    for (const [k, v] of Object.entries(grant.headers || {})) {
      // content-length·host는 브라우저가 직접 채운다 (설정하면 오류)
      if (/^(content-length|host)$/i.test(k)) continue;
      xhr.setRequestHeader(k, v);
    }
    xhr.upload.onprogress = e => { if (e.lengthComputable) onProgress?.(e.loaded / e.total); };
    xhr.onload = () => (xhr.status >= 200 && xhr.status < 300
      ? resolve()
      : reject(new ApiError('파일을 저장소에 올리지 못했어요. 잠시 후 다시 시도해 주세요.', xhr.status, 'UPLOAD_PUT_FAILED')));
    xhr.onerror = () => reject(new ApiError('파일 업로드 중 연결이 끊겼어요. 네트워크를 확인해 주세요.', 0, 'NETWORK'));
    xhr.onabort = () => reject(new ApiError('업로드를 취소했어요.', 0, 'ABORTED'));
    signal?.addEventListener('abort', () => xhr.abort());
    xhr.send(file);
  });
}
