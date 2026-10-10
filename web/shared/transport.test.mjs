import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { brotliDecompressSync } from 'node:zlib';
import { backendOrigin, passThroughEncoding, readLimitedBody, serveAssets } from './transport.js';
import { compressAssets } from './compress-assets.mjs';
import studio, { resetMaintenanceCache } from '../studio/worker.js';
import admin from '../admin/worker.js';
import landing from '../landing/src/worker.js';
import survey from '../survey/src/worker.js';

test('HTTP is refused before API, database or asset access on every public Worker', async () => {
  for (const [worker, host] of [[studio, 'studio'], [admin, 'admin'], [landing, 'www'], [survey, 'survey']]) {
    for (const path of ['/', '/api/me', '/assets/app.js']) {
      const response = await worker.fetch(new Request(`http://${host}.audeniq.com${path}`), {});
      assert.equal(response.status, 403);
      assert.equal(response.headers.get('location'), null);
      assert.equal((await response.json()).error.code, 'HTTPS_REQUIRED');
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
  const req = new Request('https://studio.audeniq.com/api/me', { method: 'POST', body, duplex: 'half' });
  await assert.rejects(readLimitedBody(req, 15), RangeError);
  assert.equal(cancelled, true);
  const small = new Request('https://studio.audeniq.com/api/me', { method: 'POST', body: '한글' });
  await assert.rejects(readLimitedBody(small, 5), RangeError);
});

test('concurrent requests share one pending maintenance read', async () => {
  resetMaintenanceCache();
  let reads = 0, release;
  const ready = new Promise(resolve => { release = resolve; });
  const db = { prepare() { return { bind() { return { async all() { reads++; await ready; return { results: [] }; } }; } }; } };
  const previous = globalThis.fetch;
  globalThis.fetch = async () => Response.json({ ok: true });
  try {
    const requests = Array.from({ length: 25 }, () => studio.fetch(new Request('https://studio.audeniq.com/api/releases'), { CONTENT_DB: db, EDGE_SERVICE_SECRET: 's'.repeat(40) }));
    await Promise.resolve();
    assert.equal(reads, 1);
    release();
    for (const response of await Promise.all(requests)) assert.equal(response.status, 200);
  } finally { globalThis.fetch = previous; resetMaintenanceCache(); }
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
    const env = { ASSETS: { async fetch(request) {
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
  const seen = [];
  const env = { ASSETS: { async fetch(request) {
    seen.push(new URL(request.url).pathname);
    return seen.length === 1 ? new Response('<html>shell</html>', { headers: { 'Content-Type': 'text/html' } }) : new Response('original');
  } } };
  const response = await serveAssets(new Request('https://studio.audeniq.com/assets/app.js', { headers: { 'Accept-Encoding': 'br' } }), env);
  assert.deepEqual(seen, ['/assets/app.js.br', '/assets/app.js']);
  assert.equal(response.headers.get('content-encoding'), null);
  assert.equal(await response.text(), 'original');
});
