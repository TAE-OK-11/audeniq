import test from 'node:test';
import assert from 'node:assert/strict';
import worker from '../worker.ts';
import adminWorker from '../../admin/worker.ts';

// 공지·이벤트·서버 점검·인증 보조는 백엔드가 한다 (crates/core/tests/site_content.rs).
// Worker는 화면 서빙과 /api 전달만 — 여기서는 전달 규칙을 확인한다.
const SECRET = 's'.repeat(40);
const env = () => ({ EDGE_SERVICE_SECRET: SECRET, ASSETS: { fetch: async () => new Response('asset') } });

async function withBackend(answer: (req: Request) => Response, fn: (seen: Request[]) => Promise<void>): Promise<void> {
  const seen: Request[] = [];
  const original = globalThis.fetch;
  // proxyToBackend는 항상 Request 하나로 fetch한다
  globalThis.fetch = async req => { seen.push(req as Request); return answer(req as Request); };
  try { await fn(seen); } finally { globalThis.fetch = original; }
}

test('studio /api/*, /ready go to the backend with the service secret, site and Cloudflare IP', async () => {
  await withBackend(() => Response.json({ ok: true }), async seen => {
    const r = await worker.fetch(new Request('https://studio.audeniq.com/api/notices?x=1'), env());
    assert.equal(r.status, 200);
    assert.equal(r.headers.get('Cache-Control'), 'no-store');
    assert.equal(r.headers.get('Strict-Transport-Security'), 'max-age=31536000; includeSubDomains');
    assert.equal(new URL(seen[0].url).href, 'https://api-origin.audeniq.com/api/notices?x=1');
    assert.equal(seen[0].headers.get('x-audeniq-site'), 'studio');
    await worker.fetch(new Request('https://studio.audeniq.com/ready'), env());
    assert.equal(new URL(seen[1].url).pathname, '/ready');
    // 브라우저가 보낸 서비스 헤더·IP 위조는 버리고, Cloudflare가 준 IP만 전달한다
    await worker.fetch(new Request('https://studio.audeniq.com/api/auth/login', {
      method: 'POST',
      headers: {
        'content-type': 'application/json', 'x-csrf-token': 't', origin: 'https://studio.audeniq.com',
        'x-audeniq-service': 'forged', 'x-audeniq-site': 'admin', 'x-audeniq-client-ip': '1.1.1.1', 'x-forwarded-for': '2.2.2.2',
        'cf-connecting-ip': '203.0.113.9', 'accept-encoding': 'zstd, br, gzip', 'user-agent': 'ua',
      },
      body: '{"email":"a"}',
    }), env());
    const h = seen[2].headers;
    assert.equal(h.get('x-audeniq-service'), SECRET);
    assert.equal(h.get('x-audeniq-site'), 'studio');
    assert.equal(h.get('x-audeniq-client-ip'), '203.0.113.9');
    assert.equal(h.get('x-forwarded-for'), null);
    assert.equal(h.get('x-csrf-token'), 't');
    assert.equal(h.get('origin'), 'https://studio.audeniq.com');
    assert.equal(h.get('accept-encoding'), 'br, gzip');
    assert.equal(h.get('user-agent'), 'ua');
    assert.equal(await seen[2].text(), '{"email":"a"}');
  });
});

test('partner webhooks keep their signature headers but never our namespace or forwarded IPs', async () => {
  await withBackend(() => new Response('{}'), async seen => {
    await worker.fetch(new Request('https://studio.audeniq.com/api/partner-hooks/D-1', {
      method: 'POST', body: '{"ack":true}',
      headers: { 'x-hub-signature-256': 'sha256=sig', signature: 'custom', 'x-audeniq-service': 'attacker', 'x-forwarded-for': '192.0.2.98' },
    }), env());
    const h = seen[0].headers;
    assert.equal(h.get('x-hub-signature-256'), 'sha256=sig');
    assert.equal(h.get('signature'), 'custom');
    assert.equal(h.get('x-audeniq-service'), SECRET);
    assert.equal(h.get('x-forwarded-for'), null);
    assert.equal(await seen[0].text(), '{"ack":true}');
  });
});

test('AI assistant (MCP) calls carry their bearer token and MCP headers; other /api calls never do', async () => {
  await withBackend(() => Response.json({}), async seen => {
    const mcpHeaders = {
      authorization: 'Bearer tok', 'mcp-protocol-version': '2026-07-28', 'mcp-method': 'tools/call', 'mcp-name': 'get_release',
      'content-type': 'application/json', accept: 'application/json, text/event-stream',
    };
    await worker.fetch(new Request('https://studio.audeniq.com/api/mcp', { method: 'POST', headers: mcpHeaders, body: '{}' }), env());
    let h = seen[0].headers;
    assert.equal(h.get('authorization'), 'Bearer tok');
    assert.equal(h.get('mcp-protocol-version'), '2026-07-28');
    assert.equal(h.get('mcp-method'), 'tools/call');
    assert.equal(h.get('mcp-name'), 'get_release');
    assert.equal(h.get('accept'), 'application/json, text/event-stream');
    assert.equal(h.get('x-audeniq-service'), SECRET);
    // Token endpoint: HTTP Basic client authentication passes through
    await worker.fetch(new Request('https://studio.audeniq.com/api/oauth/token', {
      method: 'POST', headers: { authorization: 'Basic abc', 'content-type': 'application/x-www-form-urlencoded' }, body: 'grant_type=x',
    }), env());
    assert.equal(seen[1].headers.get('authorization'), 'Basic abc');
    // Elsewhere an Authorization header is dropped (the API is cookie-authenticated)
    await worker.fetch(new Request('https://studio.audeniq.com/api/orgs', { headers: mcpHeaders }), env());
    h = seen[2].headers;
    assert.equal(h.get('authorization'), null);
    assert.equal(h.get('mcp-method'), null);
    await worker.fetch(new Request('https://studio.audeniq.com/api/oauth/authorize', { headers: { authorization: 'Bearer tok' } }), env());
    assert.equal(seen[3].headers.get('authorization'), null);
  });
});

test('OAuth discovery documents at /.well-known come from the backend; other /.well-known paths stay assets', async () => {
  await withBackend(() => Response.json({ issuer: 'https://studio.audeniq.com' }), async seen => {
    for (const path of ['/.well-known/oauth-authorization-server', '/.well-known/oauth-protected-resource', '/.well-known/oauth-protected-resource/api/mcp']) {
      const r = await worker.fetch(new Request(`https://studio.audeniq.com${path}`), env());
      assert.equal(r.status, 200);
      assert.equal(new URL(seen.at(-1)!.url).pathname, path);
      assert.equal(seen.at(-1)!.headers.get('x-audeniq-service'), SECRET);
    }
    const before = seen.length;
    const r = await worker.fetch(new Request('https://studio.audeniq.com/.well-known/security.txt'), env());
    assert.equal(await r.text(), 'asset');
    const post = await worker.fetch(new Request('https://studio.audeniq.com/.well-known/oauth-authorization-server', { method: 'POST', body: '{}' }), env());
    assert.equal(await post.text(), 'asset');
    assert.equal(seen.length, before);
  });
});

test('backend maintenance answers pass through unchanged; a dead backend is 502; no secret is 503', async () => {
  const maintenance = { error: { code: 'MAINTENANCE', message: '점검' }, maintenance: { title: '점검' } };
  await withBackend(() => Response.json(maintenance, { status: 503, headers: { 'Retry-After': '60', 'Cache-Control': 'no-store' } }), async () => {
    const r = await worker.fetch(new Request('https://studio.audeniq.com/api/orgs'), env());
    assert.equal(r.status, 503);
    assert.equal(r.headers.get('Retry-After'), '60');
    assert.deepEqual(await r.json(), maintenance);
  });
  await withBackend(() => { throw new Error('down'); }, async () => {
    const r = await worker.fetch(new Request('https://studio.audeniq.com/api/me'), env());
    assert.equal(r.status, 502);
    assert.equal((await r.json()).error.code, 'BACKEND_UNAVAILABLE');
  });
  await withBackend(() => { throw new Error('must not be called'); }, async seen => {
    assert.equal((await worker.fetch(new Request('https://studio.audeniq.com/api/me'), { ASSETS: env().ASSETS })).status, 503);
    assert.equal(seen.length, 0);
  });
});

test('admin /api/* goes to the backend as the admin site; the backend picks the allowed paths', async () => {
  await withBackend(() => Response.json({ ok: true }, { headers: { 'Access-Control-Allow-Origin': '*' } }), async seen => {
    const r = await adminWorker.fetch(new Request('https://admin.audeniq.com/api/content/notices', {
      method: 'POST', body: '{}', headers: { origin: 'https://admin.audeniq.com', 'content-type': 'application/json' },
    }), env());
    assert.equal(r.status, 200);
    assert.equal(r.headers.get('Access-Control-Allow-Origin'), null);
    assert.equal(seen[0].headers.get('x-audeniq-site'), 'admin');
    assert.equal(seen[0].headers.get('origin'), 'https://admin.audeniq.com');
    assert.equal(new URL(seen[0].url).pathname, '/api/content/notices');
  });
});

test('other paths go to assets; screen paths fall back to index.html when assets answer 404', async () => {
  assert.equal(await (await worker.fetch(new Request('https://studio.audeniq.com/notices/abc'), env())).text(), 'asset');
  const assets = { fetch: async (req: Request) => {
    const p = new URL(req.url).pathname;
    return p === '/' ? new Response('<html>', { headers: { 'Content-Security-Policy': "default-src 'self'" } }) : new Response('nf', { status: 404 });
  } };
  const e = { ...env(), ASSETS: assets };
  const r = await worker.fetch(new Request('https://studio.audeniq.com/notices/abc'), e);
  assert.equal(r.status, 200);
  assert.equal(await r.text(), '<html>');
  assert.equal(r.headers.get('Content-Security-Policy'), "default-src 'self'");
  assert.equal(r.headers.get('Cache-Control'), 'no-cache');
  assert.equal((await worker.fetch(new Request('https://studio.audeniq.com/missing.js'), e)).status, 404);
});
