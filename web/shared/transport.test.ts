import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { brotliDecompressSync } from 'node:zlib';
import { backendOrigin, passThroughEncoding, readLimitedBody, serveAssets } from './transport.ts';
import { compressAssets } from './compress-assets.ts';
import studio from '../studio/worker.ts';
import admin from '../admin/worker.ts';
import landing from '../landing/src/worker.ts';
import survey from '../survey/src/worker.ts';

/** 공개 Worker의 fetch — env 모양은 Worker마다 달라서 여기서는 묻지 않는다 */
interface PublicWorker { fetch(request: Request, env: never): Promise<Response> }
interface ErrorBody { error: { code: string } }

test('HTTP is refused before API, database or asset access on every public Worker', async () => {
  const workers: [PublicWorker, string][] = [[studio, 'studio'], [admin, 'admin'], [landing, 'www'], [survey, 'survey']];
  for (const [worker, host] of workers) {
    for (const path of ['/', '/api/me', '/assets/app.js']) {
      // HTTPS 거절은 env를 읽기 전이라 빈 env로 충분하다
      const response = await worker.fetch(new Request(`http://${host}.audeniq.com${path}`), {} as never);
      assert.equal(response.status, 403);
      assert.equal(response.headers.get('location'), null);
      assert.equal((await response.json() as ErrorBody).error.code, 'HTTPS_REQUIRED');
    }
  }
});

test('backend URLs reject plaintext, credentials and URL suffixes', () => {
  assert.equal(backendOrigin({}), 'https://api-origin.audeniq.com');
  for (const url of ['http://backend.example', 'https://u:p@backend.example', 'https://backend.example/api', 'https://backend.example/?q=1', 'https://backend.example/#x']) {
    assert.throws(() => backendOrigin({ BACKEND_URL: url }));
  }
});

test('compression honours q=0, priorities, wildcards and invalid weights', () => {
  assert.equal(passThroughEncoding('zstd, br, gzip'), 'br, gzip');
  assert.equal(passThroughEncoding('br;q=0, gzip;q=0.5'), 'gzip;q=0.5');
  assert.equal(passThroughEncoding('br;q=wat, gzip;q=2'), 'identity');
  assert.equal(passThroughEncoding('br;q=0, *;q=0.7'), 'gzip;q=0.7');
  assert.equal(passThroughEncoding('br;q=0, br'), 'identity');
  assert.equal(passThroughEncoding(null), 'identity');
});

test('oversized streamed JSON stops reading and cancels the source by byte count', async () => {
  let cancelled = false;
  const body = new ReadableStream({ pull(controller) { controller.enqueue(new Uint8Array(10)); }, cancel() { cancelled = true; } });
  const init: RequestInit & { duplex: 'half' } = { method: 'POST', body, duplex: 'half' };
  const req = new Request('https://studio.audeniq.com/api/me', init);
  await assert.rejects(readLimitedBody(req, 15), RangeError);
  assert.equal(cancelled, true);
  const small = new Request('https://studio.audeniq.com/api/me', { method: 'POST', body: '한글' });
  await assert.rejects(readLimitedBody(small, 5), RangeError);
});

test('build-time Brotli is served unchanged with the original media type and variant headers', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'audeniq-br-'));
  try {
    const raw = Buffer.from('const 한국어 = "AUDENIQ";\n'.repeat(500));
    writeFileSync(join(dir, 'app.js'), raw);
    const stats = compressAssets(dir);
    assert.equal(stats.files, 1);
    assert.ok(stats.compressedBytes < stats.rawBytes / 10);
    const bytes = readFileSync(join(dir, 'app.js.br'));
    let calls = 0;
    const env = { ASSETS: { async fetch(request: Request) {
      calls++;
      assert.equal(new URL(request.url).pathname, '/assets/app.js.br');
      return new Response(bytes, { headers: { 'Content-Type': 'application/octet-stream', ETag: '"br-variant"' } });
    } } };
    const response = await serveAssets(new Request('https://studio.audeniq.com/assets/app.js', { headers: { 'Accept-Encoding': 'zstd, br, gzip' } }), env);
    assert.equal(calls, 1);
    assert.equal(response.headers.get('content-type'), 'application/javascript; charset=utf-8');
    assert.equal(response.headers.get('content-encoding'), 'br');
    assert.equal(response.headers.get('vary'), 'Accept-Encoding');
    assert.equal(response.headers.get('etag'), '"br-variant"');
    assert.deepEqual(brotliDecompressSync(Buffer.from(await response.arrayBuffer())), raw);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test('missing Brotli sidecars fall back to the original asset without a false encoding', async () => {
  const seen: string[] = [];
  const env = { ASSETS: { async fetch(request: Request) {
    seen.push(new URL(request.url).pathname);
    return seen.length === 1 ? new Response('<html>shell</html>', { headers: { 'Content-Type': 'text/html' } }) : new Response('original');
  } } };
  const response = await serveAssets(new Request('https://studio.audeniq.com/assets/app.js', { headers: { 'Accept-Encoding': 'br' } }), env);
  assert.deepEqual(seen, ['/assets/app.js.br', '/assets/app.js']);
  assert.equal(response.headers.get('content-encoding'), null);
  assert.equal(await response.text(), 'original');
});

test('a missing asset answered with the SPA shell is a 404, never HTML as a script', async () => {
  const shell = () => new Response('<html>shell</html>', { headers: { 'Content-Type': 'text/html; charset=utf-8' } });
  const env = { ASSETS: { fetch: async () => shell() } };
  for (const path of ['/assets/Releases-OLDHASH.js', '/assets/index-OLD.css', '/assets/missing.js.map']) {
    const r = await serveAssets(new Request(`https://studio.audeniq.com${path}`, { headers: { 'Accept-Encoding': 'br' } }), env);
    assert.equal(r.status, 404, path);
    assert.equal(r.headers.get('cache-control'), 'no-store');
  }
  // Screens (no extension) and the shell itself still get the HTML.
  for (const path of ['/', '/releases', '/index.html']) {
    const r = await serveAssets(new Request(`https://studio.audeniq.com${path}`), env);
    assert.equal(r.status, 200, path);
  }
  // Studio is served by the backend now, with the same rule (crates/core/src/studio_static.rs).
});
