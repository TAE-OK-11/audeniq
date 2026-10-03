// Public network policy. Private Compose services continue to use HTTP.
export function requireHttps(request) {
  const url = new URL(request.url);
  if (url.protocol === 'https:' || (url.protocol === 'http:' && ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname))) return null;
  return Response.json({ error: { code: 'HTTPS_REQUIRED' } }, {
    status: 403, headers: { 'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff' },
  });
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
