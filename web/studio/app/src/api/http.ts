// 실제 API 호출 계층 — CloudFront + audeniq-studio-edge Function을 거쳐 메인 서버로 전달된다.
// 브라우저는 세션 쿠키(HttpOnly)와 CSRF 토큰만 다루고, 서비스 비밀값은 엣지만 가진다.
import { ApiError, messageForCode } from './errors';
import { clearAsyncCache } from '../lib/asyncCache';

export const API_BASE = (import.meta.env.VITE_API_BASE ?? '').replace(/\/$/, '');

/** 서버가 준 '/api/…' 주소(보관된 커버 이미지 등)를 API 주소로 */
export function apiUrl(src: string | null | undefined): string {
  if (!src) return '';
  return src.startsWith('/api/') ? `${API_BASE}${src}` : src;
}

let csrfToken = '';

// 엣지·게이트웨이가 직접 돌려주는 오류(본문이 JSON이 아님)를 상태 코드로 분류.
// 2026-10-28 수정: 502/503/504를 전부 DATABASE_UNAVAILABLE로 합치지 않는다.
// 백엔드가 준 코드가 있으면 그 코드가 우선이고, 코드가 없을 때만 이 표를 쓴다.
// - 502: 상류(게이트웨이) 불가, 504: 상류 타임아웃, 503 코드 없음: 서비스 불가.
// - 500은 이전 표에 없어 서버 장애로 분류되지 않았으므로 INTERNAL_ERROR로 추가한다.
const STATUS_CODES: Record<number, string> = {
  401: 'UNAUTHENTICATED', 403: 'FORBIDDEN', 404: 'NOT_FOUND', 413: 'PAYLOAD_TOO_LARGE',
  429: 'RATE_LIMITED', 500: 'INTERNAL_ERROR', 502: 'UPSTREAM_UNAVAILABLE',
  503: 'SERVICE_UNAVAILABLE', 504: 'REQUEST_TIMEOUT',
};
let currentOrgId = '';

export function setCsrf(token: string) { csrfToken = token; }
/** 이 탭이 로그인한 사용자 (다른 탭에서 다른 계정으로 로그인했는지 알아보는 데 쓴다) */
let sessionUser = '';
export function setSessionUser(id: string) { sessionUser = id; }
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
  method?: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE';
  body?: unknown;
  timeoutMs?: number;
  /** 401이어도 로그인 화면으로 보내지 않음 (세션 확인·로그인 요청용) */
  quiet401?: boolean;
  /** 서버 장애여도 전역 오류 창을 띄우지 않음 (화면에서 직접 안내하는 요청용) */
  quietServer?: boolean;
  /** 내부용: CSRF 토큰을 새로 받아 한 번 다시 보낸 요청 */
  csrfRetried?: boolean;
  signal?: AbortSignal;
}

/** 지금 받고 있는 GET — 같은 주소를 여러 곳에서 동시에 부르면(화면과 머리글이 같은 목록을 부르는 등) 한 번만 보낸다 */
const inflight = new Map<string, Promise<unknown>>();

/**
 * JSON 요청. 상태 변경 요청은 CSRF 토큰을 붙이고, 본문이 없으면 `{}`를 보낸다
 * (백엔드는 변경 요청에 JSON 본문을 요구한다).
 * 같은 GET이 이미 가고 있으면 그 응답을 함께 기다린다. 바꾸는 요청이 끝나면 화면 데이터 캐시를 비운다(lib/asyncCache).
 */
export function req<T>(path: string, opts: ReqOptions = {}): Promise<T> {
  const method = opts.method ?? 'GET';
  if (method !== 'GET') return send<T>(path, opts).finally(clearAsyncCache);
  if (opts.signal) return send<T>(path, opts);
  // 401·서버 오류를 어떻게 알릴지가 다른 요청은 따로 보낸다
  const k = `${path}|${opts.quiet401 ? 1 : 0}${opts.quietServer ? 1 : 0}${opts.timeoutMs ?? ''}`;
  const hit = inflight.get(k);
  if (hit) return hit as Promise<T>;
  const p = send<T>(path, opts).finally(() => inflight.delete(k));
  inflight.set(k, p);
  return p;
}

async function send<T>(path: string, opts: ReqOptions): Promise<T> {
  const method = opts.method ?? 'GET';
  // 토큰을 받고 있는 중이면 바꾸는 요청은 그 토큰을 기다렸다가 붙인다 (bootstrapCsrf는 기다리지 않고 시작한다)
  if (method !== 'GET' && csrfPending && path !== CSRF_PATH) await csrfPending.catch(() => {});
  const headers: Record<string, string> = { Accept: 'application/json' };
  let body: string | undefined;
  if (method !== 'GET') {
    headers['Content-Type'] = 'application/json';
    body = JSON.stringify(opts.body ?? {});
    if (csrfToken) headers['X-CSRF-Token'] = csrfToken;
  }
  const ctrl = new AbortController();
  const timer = window.setTimeout(() => ctrl.abort(), opts.timeoutMs ?? 20000);
  const abort = () => ctrl.abort();
  opts.signal?.addEventListener('abort', abort, { once: true });
  if (opts.signal?.aborted) ctrl.abort();
  let res: Response;
  let text: string;
  try {
    res = await fetch(`${API_BASE}${path}`, { method, headers, body, credentials: 'include', signal: ctrl.signal, cache: 'no-store' });
    // Keep the deadline and caller cancellation active through body consumption.
    text = await res.text();
  } catch (e) {
    const aborted = e instanceof DOMException && e.name === 'AbortError';
    // 기기가 온라인인데 연결 자체가 안 되면 서버 쪽 문제로 본다 (오프라인은 SystemStatus가 따로 알린다)
    if (!aborted && !opts.quietServer && navigator.onLine && !opts.signal?.aborted) reportServerIssue({ kind: 'down', status: 0, code: 'NETWORK' });
    throw new ApiError(aborted ? '서버 응답이 늦어요. 잠시 후 다시 시도해 주세요.' : '네트워크 연결을 확인해 주세요.', 0, aborted ? 'TIMEOUT' : 'NETWORK');
  } finally {
    window.clearTimeout(timer);
    opts.signal?.removeEventListener('abort', abort);
  }

  let data: unknown = null;
  if (text) {
    try { data = JSON.parse(text); } catch { data = null; }
  }

  if (!res.ok) {
    const code = (data as { error?: { code?: string } } | null)?.error?.code
      ?? STATUS_CODES[res.status] ?? '';
    // 다른 탭에서 다시 로그인하면 세션(과 CSRF 토큰)이 바뀌어 이 탭의 쓰기가 모두 403이 된다.
    // 토큰을 새로 받아 한 번만 다시 보낸다. 다른 계정으로 바뀌었으면 데이터가 섞이지 않게 새로 연다.
    if (res.status === 403 && method !== 'GET' && !opts.csrfRetried && path !== '/api/auth/csrf') {
      const refreshed = await refreshSession(headers['X-CSRF-Token'] ?? '');
      if (refreshed) return req<T>(path, { ...opts, csrfRetried: true });
    }
    if (res.status === 401 && !opts.quiet401) {
      csrfToken = '';
      const onAuthPage = /^\/(login|signup|find-account)/.test(currentPath());
      if (!onAuthPage) window.location.assign(toHref('/login'));
    }
    const issue = classifyServerIssue(res.status, code);
    if (issue?.kind === 'maintenance') issue.window = (data as { maintenance?: unknown } | null)?.maintenance;
    // 점검은 조용한 요청이어도 알린다 (어느 요청이든 먼저 받은 쪽이 점검 화면을 띄운다)
    if (issue && (!opts.quietServer || issue.kind === 'maintenance')) reportServerIssue(issue);
    const msg = code === 'PAYLOAD_TOO_LARGE' ? '보낼 정보가 너무 커요. 앨범 소개나 가사 길이를 줄여 주세요.' : messageForCode(code, res.status);
    const server = (data as { error?: ApiError['server'] } | null)?.error;
    throw new ApiError(msg, res.status, code, server ? { message: server.message, details: server.details } : undefined);
  }
  return (data ?? {}) as T;
}

/** 세션이 바뀌었는지 확인하고 CSRF 토큰을 새로 받는다. 다시 보내도 되면 true */
async function refreshSession(sentWith: string): Promise<boolean> {
  // 다른 요청이 이미 새 토큰을 받아 왔으면 그걸로 다시 보낸다 — 서버 토큰은 세션마다 같아서
  // 새로 받아도 '바뀌지 않음'으로 보여 두 번째 403이 그대로 오류가 됐다
  if (csrfToken && csrfToken !== sentWith) return true;
  try {
    const me = await req<{ user_id: string }>('/api/me', { quiet401: true, quietServer: true });
    if (sessionUser && me.user_id !== sessionUser) {
      window.location.reload();
      return false;
    }
    await bootstrapCsrf();
  } catch {
    return false;
  }
  return csrfToken !== '' && csrfToken !== sentWith;
}

const CSRF_PATH = '/api/auth/csrf';
/** 받고 있는 CSRF 토큰 — 여러 곳에서 동시에 불러도 한 번만 받는다 */
let csrfPending: Promise<void> | null = null;

/** 새로고침 후 세션 쿠키로 CSRF 토큰을 다시 받는다 */
export function bootstrapCsrf(): Promise<void> {
  csrfPending ??= req<{ csrf_token: string }>(CSRF_PATH, { method: 'POST', quiet401: true })
    .then(r => { csrfToken = r.csrf_token; })
    .finally(() => { csrfPending = null; });
  return csrfPending;
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
 * 서명된 URL로 R2에 파일을 직접 올린다 (음원 원본은 CloudFront를 거치지 않음).
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
      // 서명 URL은 한 번만 쓸 수 있다 (If-None-Match: *). 412면 이미 누군가 올린 링크라
      // 등록하지 않고, 파일을 다시 골라 새 링크를 받게 한다.
      : xhr.status === 412
        ? reject(new ApiError('이미 사용된 업로드 링크예요. 파일을 다시 선택해 주세요.', 412, 'UPLOAD_GRANT_USED'))
        : reject(new ApiError('파일을 저장소에 올리지 못했어요. 잠시 후 다시 시도해 주세요.', xhr.status, 'UPLOAD_PUT_FAILED')));
    xhr.onerror = () => reject(new ApiError('파일 업로드 중 연결이 끊겼어요. 네트워크를 확인해 주세요.', 0, 'NETWORK'));
    xhr.onabort = () => reject(new ApiError('업로드를 취소했어요.', 0, 'ABORTED'));
    // 이미 취소된 뒤면 보내지 않는다 — 'abort' 이벤트는 다시 오지 않아 파일 전체를 올리고 있었다
    if (signal?.aborted) { reject(new ApiError('업로드를 취소했어요.', 0, 'ABORTED')); return; }
    const stop = () => xhr.abort();
    signal?.addEventListener('abort', stop, { once: true });
    xhr.onloadend = () => signal?.removeEventListener('abort', stop);
    xhr.send(file);
  });
}

/** 서버가 만든 파일(CSV 등)을 받아 저장한다. 오류는 JSON 요청과 같은 문구로 알린다. */
export async function downloadFile(path: string, fallbackName: string): Promise<void> {
  let res: Response;
  try {
    res = await fetch(`${API_BASE}${path}`, { credentials: 'include', cache: 'no-store' });
  } catch {
    if (navigator.onLine) reportServerIssue({ kind: 'down', status: 0, code: 'NETWORK' });
    throw new ApiError('네트워크 연결을 확인해 주세요.', 0, 'NETWORK');
  }
  if (!res.ok) {
    let data: { error?: { code?: string }; maintenance?: unknown } | null = null;
    try { data = await res.json(); } catch { /* 본문 없음 */ }
    const code = data?.error?.code ?? STATUS_CODES[res.status] ?? '';
    const issue = classifyServerIssue(res.status, code);
    if (issue?.kind === 'maintenance') issue.window = data?.maintenance;
    if (issue) reportServerIssue(issue);
    throw new ApiError(messageForCode(code, res.status), res.status, code);
  }
  const name = /filename="([^"]+)"/.exec(res.headers.get('Content-Disposition') ?? '')?.[1] ?? fallbackName;
  let blob: Blob;
  try {
    blob = await res.blob();
  } catch {
    if (navigator.onLine) reportServerIssue({ kind: 'down', status: 0, code: 'NETWORK' });
    throw new ApiError('네트워크 연결을 확인해 주세요.', 0, 'NETWORK');
  }
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
}
