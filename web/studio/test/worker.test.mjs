import test from 'node:test';
import assert from 'node:assert/strict';
import worker from '../worker.js';
import adminWorker from '../../admin/worker.js';

// 공지·이벤트·서버 점검·인증 보조는 백엔드가 한다 (crates/core/tests/site_content.rs).
// Worker는 화면 서빙과 /api 전달만 — 여기서는 전달 규칙을 확인한다.
const SECRET = 's'.repeat(40);
const env = () => ({ EDGE_SERVICE_SECRET: SECRET, ASSETS: { fetch: async () => new Response('asset') } });

async function withBackend(answer, fn) {
  const seen = [];
  const original = globalThis.fetch;
  globalThis.fetch = async req => { seen.push(req); return answer(req); };
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
  const assets = { fetch: async req => {
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
