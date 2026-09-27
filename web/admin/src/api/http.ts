// API 호출 계층 — 관리자 Worker(worker.js)가 같은 출처의 /api/*를 백엔드로 전달한다.
// 브라우저는 세션 쿠키(HttpOnly)와 CSRF 토큰만 다루고, 서비스 비밀값은 Worker만 가진다.
// (스튜디오 web/studio/app/src/api/http.ts와 같은 규칙)
import { ApiError, messageForCode } from './errors';
import { currentPath, toHref } from '../lib/router';

const API_BASE = (import.meta.env.VITE_API_BASE ?? '').replace(/\/$/, '');

let csrfToken = '';

// Worker가 직접 돌려주는 오류(본문이 JSON이 아님)를 상태 코드로 분류
const STATUS_CODES: Record<number, string> = {
  401: 'UNAUTHENTICATED', 403: 'FORBIDDEN', 404: 'NOT_FOUND', 413: 'PAYLOAD_TOO_LARGE',
  429: 'RATE_LIMITED', 502: 'BACKEND_UNAVAILABLE', 503: 'DATABASE_UNAVAILABLE', 504: 'DATABASE_UNAVAILABLE',
};

export function setCsrf(token: string) { csrfToken = token; }
export function hasCsrf() { return !!csrfToken; }

interface ReqOptions {
  method?: 'GET' | 'POST' | 'PUT' | 'DELETE';
  body?: unknown;
  timeoutMs?: number;
  /** 401이어도 로그인 화면으로 보내지 않음 (세션 확인·로그인 요청용) */
  quiet401?: boolean;
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
  let res: Response;
  try {
    res = await fetch(`${API_BASE}${path}`, { method, headers, body, credentials: 'include', signal: ctrl.signal, cache: 'no-store' });
  } catch (e) {
    const aborted = e instanceof DOMException && e.name === 'AbortError';
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
    const code = (data as { error?: { code?: string } } | null)?.error?.code ?? STATUS_CODES[res.status] ?? '';
    if (res.status === 401 && !opts.quiet401) {
      csrfToken = '';
      if (!currentPath().startsWith('/login')) window.location.assign(toHref('/login'));
    }
    const msg = code === 'MAINTENANCE'
      ? ((data as { error?: { message?: string } } | null)?.error?.message || messageForCode(code, res.status))
      : messageForCode(code, res.status);
    throw new ApiError(msg, res.status, code);
  }
  return (data ?? {}) as T;
}

/** 새로고침 후 세션 쿠키로 CSRF 토큰을 다시 받는다 */
export async function bootstrapCsrf(): Promise<void> {
  const r = await req<{ csrf_token: string }>('/api/auth/csrf', { method: 'POST', quiet401: true });
  csrfToken = r.csrf_token;
}
