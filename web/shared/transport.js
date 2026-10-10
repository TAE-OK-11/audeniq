// Public network policy. Private Compose services continue to use HTTP.
export function requireHttps(request) {
  const url = new URL(request.url);
  if (url.protocol === 'https:' || (url.protocol === 'http:' && ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname))) return null;
  return Response.json({ error: { code: 'HTTPS_REQUIRED' } }, {
    status: 403, headers: { 'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff' },
  });
}

/** 길이가 같을 때만 비교하는 상수 시간 문자열 비교 */
export function safeEqual(a, b) {
  if (typeof a !== 'string' || typeof b !== 'string' || a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

// AWS CloudFront가 화면(S3)을 서빙하고 /api/* 만 이 Worker로 보낼 때의 설정 (deploy/aws/README.md)
//   EDGE_ORIGIN_SECRET  (secret) CloudFront origin custom header(x-audeniq-edge)에 넣은 값.
//                       이 값이 맞는 요청만 CloudFront 경유로 보고 CloudFront-Viewer-Address를 사용자 IP로 믿는다
//   EDGE_ORIGIN_ONLY    (var) "true"면 CloudFront를 거치지 않은 직접 호출을 403으로 막는다 (전환이 끝난 뒤 켠다)
//   PUBLIC_ORIGIN       (var) 브라우저가 보는 주소 (예: https://studio.audeniq.com). Origin 검사·Turnstile hostname 기준
// 셋 다 없으면 지금처럼 Worker가 직접 도메인을 받는 것으로 본다. 미리 넣어 둬도 직접 접속은 그대로 동작한다.
export const EDGE_HEADER = 'x-audeniq-edge';

export function fromEdge(request, env) {
  const secret = env?.EDGE_ORIGIN_SECRET;
  return typeof secret === 'string' && secret.length >= 32 && safeEqual(request.headers.get(EDGE_HEADER) ?? '', secret);
}

export function requireEdge(request, env) {
  if (!['1', 'true', 'yes', 'on'].includes(String(env?.EDGE_ORIGIN_ONLY ?? '').trim().toLowerCase())) return null;
  if (fromEdge(request, env)) return null;
  return Response.json({ error: { code: 'FORBIDDEN' } }, {
    status: 403, headers: { 'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff' },
  });
}

export function publicOrigin(request, env) {
  if (!env?.PUBLIC_ORIGIN) return new URL(request.url).origin;
  const url = new URL(env.PUBLIC_ORIGIN);
  if (url.protocol !== 'https:' || url.username || url.password || url.search || url.hash || url.pathname !== '/') {
    throw new TypeError('PUBLIC_ORIGIN must be an HTTPS origin');
  }
  return url.origin;
}

/** 사용자 IP. CloudFront 경유(비밀 확인됨)면 CloudFront-Viewer-Address, 아니면 Cloudflare가 덮어쓰는 CF-Connecting-IP */
export function clientIp(request, env) {
  if (fromEdge(request, env)) {
    // "203.0.113.7:4312" 또는 "2001:db8::1:4312" — 마지막 콜론 뒤는 포트
    const address = request.headers.get('cloudfront-viewer-address') ?? '';
    const cut = address.lastIndexOf(':');
    return cut > 0 ? address.slice(0, cut).replace(/^\[|\]$/g, '') : null;
  }
  return request.headers.get('cf-connecting-ip');
}

export function backendOrigin(env) {
  const url = new URL(env.BACKEND_URL || 'https://api-origin.audeniq.com');
  if (url.protocol !== 'https:' || url.username || url.password || url.search || url.hash || url.pathname !== '/') {
    throw new TypeError('BACKEND_URL must be an HTTPS origin');
  }
  return url.origin;
}

export function encodingQuality(accept, encoding) {
  const values = new Map();
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
export function passThroughEncoding(accept) {
  return ['br', 'gzip'].filter(name => encodingQuality(accept, name) > 0)
    .map(name => {
      const q = encodingQuality(accept, name);
      return q === 1 ? name : `${name};q=${q}`;
    }).join(', ') || 'identity';
}

export function clientEncoding(request) {
  return request.cf?.clientAcceptEncoding ?? request.headers.get('accept-encoding');
}

export function secureResponse(response) {
  const headers = new Headers(response.headers);
  headers.set('Strict-Transport-Security', 'max-age=31536000; includeSubDomains');
  return new Response(response.body, {
    status: response.status, statusText: response.statusText, headers,
    ...(headers.has('content-encoding') ? { encodeBody: 'manual' } : {}),
  });
}

const ASSET_TYPES = {
  js: 'application/javascript; charset=utf-8', css: 'text/css; charset=utf-8',
  html: 'text/html; charset=utf-8', json: 'application/json; charset=utf-8',
  svg: 'image/svg+xml', wasm: 'application/wasm',
};

export async function serveAssets(request, env) {
  const url = new URL(request.url);
  const path = url.pathname === '/' ? '/index.html' : url.pathname;
  const type = ASSET_TYPES[path.split('.').pop()];
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
      return new Response(br.body, { status: br.status, headers, encodeBody: 'manual' });
    }
  }
  return env.ASSETS.fetch(request);
}

export async function readLimitedBody(request, maxBytes) {
  const declared = request.headers.get('content-length');
  if (declared && Number(declared) > maxBytes) throw new RangeError('body too large');
  const reader = request.body?.getReader();
  if (!reader) return new Uint8Array();
  const parts = [];
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
