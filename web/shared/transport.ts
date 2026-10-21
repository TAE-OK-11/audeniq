import type { TransportEnv } from './worker-types.ts';

/** Workers가 요청에 붙이는 정보 (request.cf) 중 여기서 읽는 것 */
interface CfProperties { clientAcceptEncoding?: string }
/** Workers 요청 — 로컬·테스트의 일반 Request에는 cf가 없다 */
export type WorkerRequest = Request & { cf?: CfProperties };
/** Workers 전용 응답 옵션: encodeBody 'manual'이면 이미 압축된 본문을 그대로 보낸다 */
type WorkerResponseInit = ResponseInit & { encodeBody?: 'automatic' | 'manual' };
/** 스트림 본문 요청 (fetch 표준의 duplex — lib.dom에는 아직 없다) */
type StreamingRequestInit = RequestInit & { duplex?: 'half' };

// Public network policy. Private Compose services continue to use HTTP.
export function requireHttps(request: Request): Response | null {
  const url = new URL(request.url);
  if (url.protocol === 'https:' || (url.protocol === 'http:' && ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname))) return null;
  return Response.json({ error: { code: 'HTTPS_REQUIRED' } }, {
    status: 403, headers: { 'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff' },
  });
}

export function backendOrigin(env: Pick<TransportEnv, 'BACKEND_URL'>): string {
  const url = new URL(env.BACKEND_URL || 'https://api-origin.audeniq.com');
  if (url.protocol !== 'https:' || url.username || url.password || url.search || url.hash || url.pathname !== '/') {
    throw new TypeError('BACKEND_URL must be an HTTPS origin');
  }
  return url.origin;
}

export function encodingQuality(accept: string | null | undefined, encoding: string): number {
  const values = new Map<string, number>();
  for (const part of String(accept ?? '').toLowerCase().split(',')) {
    const [name, ...parameters] = part.trim().split(';').map(s => s.trim());
    let q = 1;
    for (const parameter of parameters) {
      if (!/^q\s*=/.test(parameter)) continue;
      const raw = parameter.replace(/^q\s*=\s*/, '');
      q = /^(?:0(?:\.\d{0,3})?|1(?:\.0{0,3})?)$/.test(raw) ? Number(raw) : 0;
    }
    // Conflicting duplicate declarations are treated conservatively.
    values.set(name, Math.min(values.get(name) ?? 1, q));
  }
  return values.get(encoding) ?? values.get('*') ?? 0;
}

// Workers Fetch supports br/gzip. The CDN's compression rule negotiates zstd
// with visitors; forwarding unsupported zstd to Fetch can break the body.
export function passThroughEncoding(accept: string | null | undefined): string {
  return ['br', 'gzip'].filter(name => encodingQuality(accept, name) > 0)
    .map(name => {
      const q = encodingQuality(accept, name);
      return q === 1 ? name : `${name};q=${q}`;
    }).join(', ') || 'identity';
}

export function clientEncoding(request: WorkerRequest): string | null {
  return request.cf?.clientAcceptEncoding ?? request.headers.get('accept-encoding');
}

export function secureResponse(response: Response): Response {
  const headers = new Headers(response.headers);
  headers.set('Strict-Transport-Security', 'max-age=31536000; includeSubDomains');
  const init: WorkerResponseInit = {
    status: response.status, statusText: response.statusText, headers,
    ...(headers.has('content-encoding') ? { encodeBody: 'manual' } : {}),
  };
  return new Response(response.body, init);
}

const ASSET_TYPES: Partial<Record<string, string>> = {
  js: 'application/javascript; charset=utf-8', css: 'text/css; charset=utf-8',
  html: 'text/html; charset=utf-8', json: 'application/json; charset=utf-8',
  svg: 'image/svg+xml', wasm: 'application/wasm',
};

export async function serveAssets(request: WorkerRequest, env: Pick<TransportEnv, 'ASSETS'>): Promise<Response> {
  const url = new URL(request.url);
  const path = url.pathname === '/' ? '/index.html' : url.pathname;
  const type = ASSET_TYPES[path.split('.').pop()!];
  const accept = clientEncoding(request);
  if (type && ['GET', 'HEAD'].includes(request.method) && !request.headers.has('range')
      && encodingQuality(accept, 'br') > 0 && encodingQuality(accept, 'br') >= encodingQuality(accept, 'gzip')) {
    url.pathname = `${path}.br`;
    const br = await env.ASSETS.fetch(new Request(url, request));
    // A missing sidecar can be answered with the SPA's HTML fallback.
    if ([200, 304].includes(br.status) && !br.headers.get('content-type')?.startsWith('text/html')) {
      const headers = new Headers(br.headers);
      headers.set('Content-Type', type);
      headers.set('Content-Encoding', 'br');
      const vary = headers.get('vary');
      if (!vary?.toLowerCase().split(',').map(s => s.trim()).includes('accept-encoding')) {
        headers.set('Vary', vary ? `${vary}, Accept-Encoding` : 'Accept-Encoding');
      }
      if (path.endsWith('.html')) headers.set('Cache-Control', 'no-cache');
      const init: WorkerResponseInit = { status: br.status, headers, encodeBody: 'manual' };
      return new Response(br.body, init);
    }
  }
  const response = await env.ASSETS.fetch(request);
  // With SPA fallback the asset layer answers a missing file with the app's
  // HTML and 200. A script or stylesheet from an earlier deploy then "loads"
  // as HTML: Safari reports a MIME error instead of a failed import, and the
  // app cannot tell the user to reload. Missing files are a 404.
  const ext = /\.([a-z0-9]+)$/i.exec(url.pathname)?.[1]?.toLowerCase();
  if (ext && ext !== 'html' && response.headers.get('content-type')?.startsWith('text/html')) {
    return new Response('Not found', { status: 404, headers: { 'Content-Type': 'text/plain; charset=utf-8', 'Cache-Control': 'no-store' } });
  }
  return response;
}

export async function readLimitedBody(request: Request, maxBytes: number): Promise<Uint8Array> {
  const declared = request.headers.get('content-length');
  if (declared && Number(declared) > maxBytes) throw new RangeError('body too large');
  const reader = request.body?.getReader();
  if (!reader) return new Uint8Array();
  const parts: Uint8Array[] = [];
  let size = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      size += value.byteLength;
      if (size > maxBytes) {
        await reader.cancel().catch(() => {});
        throw new RangeError('body too large');
      }
      parts.push(value);
    }
  } finally { reader.releaseLock(); }
  const bytes = new Uint8Array(size);
  let offset = 0;
  for (const part of parts) { bytes.set(part, offset); offset += part.byteLength; }
  return bytes;
}

// ---- 백엔드 전달 (Worker는 화면 서빙 + /api 전달만; 공지·점검·인증 보조·설문은 백엔드가 한다) ----
// 브라우저 헤더는 필요한 것만 골라 보낸다. 브라우저가 보낸 x-audeniq-* 와 IP 헤더는 버리고,
// 서비스 비밀·화면 이름(x-audeniq-site, crates/core/src/sites.rs)·Cloudflare가 준 사용자 IP를 붙인다.
// user-agent: 권리 서류 서명 기록(증거)에 남긴다
const FORWARD_HEADERS = ['cookie', 'origin', 'content-type', 'accept', 'x-csrf-token', 'sec-fetch-site', 'x-request-id', 'user-agent'];
const PARTNER_HOOK = /^\/api\/partner-hooks\//;
// AI 어시스턴트(MCP, crates/core/src/mcp)의 서버 간 호출: 이 경로에만 Authorization(베어러 토큰·클라이언트 인증)과
// MCP 요청 메타데이터 헤더를 넘긴다. 그 밖의 /api는 지금처럼 쿠키만 쓴다.
const MCP_CLIENT_PATHS = new Set(['/api/mcp', '/api/mcp/', '/api/oauth/token', '/api/oauth/register', '/api/oauth/revoke', '/api/oauth/device_authorization']);
const MCP_HEADERS = ['authorization', 'mcp-protocol-version', 'mcp-method', 'mcp-name'];
/** OAuth 발견 문서 (RFC 8414·9728): 사이트 루트의 /.well-known에 있어야 하고 백엔드가 답한다 */
export const MCP_DISCOVERY_PATHS = new Set([
  '/.well-known/oauth-protected-resource',
  '/.well-known/oauth-protected-resource/api/mcp',
  '/.well-known/oauth-authorization-server',
]);
const PARTNER_DROP = new Set(['x-forwarded-for', 'x-real-ip', 'host', 'connection', 'accept-encoding']);

export function backendHeaders(request: WorkerRequest, env: Required<Pick<TransportEnv, 'EDGE_SERVICE_SECRET'>>, pathname: string, site: string): Headers {
  const src = request.headers;
  const headers = new Headers();
  for (const name of FORWARD_HEADERS) {
    const v = src.get(name);
    if (v) headers.set(name, v);
  }
  headers.set('accept-encoding', passThroughEncoding(clientEncoding(request)));
  if (MCP_CLIENT_PATHS.has(pathname)) {
    for (const name of MCP_HEADERS) {
      const v = src.get(name);
      if (v) headers.set(name, v);
    }
  }
  if (PARTNER_HOOK.test(pathname)) {
    // 파트너 설정이 정한 서명·시각 헤더(이름 자유)는 전달하되 우리 이름공간·전달 IP 헤더는 제외
    for (const [name, value] of src) {
      if (!name.startsWith('x-audeniq-') && !PARTNER_DROP.has(name)) headers.set(name, value);
    }
  }
  // 로그인 시도 제한은 사용자 IP 기준 — Cloudflare가 매 요청 덮어쓰는 CF-Connecting-IP만 믿는다
  const ip = src.get('cf-connecting-ip');
  if (ip) headers.set('x-audeniq-client-ip', ip);
  headers.set('x-audeniq-site', site);
  headers.set('x-audeniq-service', env.EDGE_SERVICE_SECRET);
  return headers;
}

const proxyError = (status: number, code: string): Response => Response.json({ error: { code } }, {
  status, headers: { 'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff' },
});

/** 요청을 백엔드(터널 → api)로 넘긴다. path를 주면 그 경로로 (설문: /api/x → /api/survey/x) */
export async function proxyToBackend(request: WorkerRequest, env: Pick<TransportEnv, 'BACKEND_URL' | 'EDGE_SERVICE_SECRET'>, { site, path }: { site: string; path?: string }): Promise<Response> {
  if (!env.EDGE_SERVICE_SECRET) return proxyError(503, 'BACKEND_UNAVAILABLE');
  const url = new URL(request.url);
  const target = path ?? url.pathname;
  const read = request.method === 'GET' || request.method === 'HEAD';
  let res: Response;
  try {
    const init: StreamingRequestInit = {
      method: request.method,
      // 위에서 EDGE_SERVICE_SECRET이 있음을 확인했다
      headers: backendHeaders(request, env as Required<Pick<TransportEnv, 'EDGE_SERVICE_SECRET'>>, target, site),
      // 본문은 읽지 않고 스트림으로 넘긴다 (Worker 메모리·CPU 절약)
      body: read ? null : request.body,
      redirect: 'manual',
      duplex: 'half',
    };
    res = await fetch(new Request(backendOrigin(env) + target + url.search, init));
  } catch {
    return proxyError(502, 'BACKEND_UNAVAILABLE');
  }
  // 본문을 건드리지 않고 헤더만 — 압축된 응답이 다시 풀리거나 재압축되지 않는다
  const init: WorkerResponseInit = { status: res.status, statusText: res.statusText, headers: res.headers,
    ...(res.headers.has('content-encoding') ? { encodeBody: 'manual' } : {}) };
  const out = new Response(res.body, init);
  if (!out.headers.has('Cache-Control')) out.headers.set('Cache-Control', 'no-store');
  return out;
}
