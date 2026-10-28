// Dependency-free local regression harness for the actual download function.
const { stripTypeScriptTypes } = require('node:module');
const { readFileSync } = require('node:fs');
const { runInNewContext } = require('node:vm');
const assert = require('node:assert/strict');
const source = readFileSync(__dirname + '/../src/api/http.ts', 'utf8');
const fn = stripTypeScriptTypes(source.slice(source.indexOf('export async function downloadFile')).replace('export ', ''));
class ApiError extends Error { constructor(message, status, code) { super(message); this.code = code; } }
async function scenario(stallBody) {
  let expire, cleared = false, signal, reports = 0;
  const pending = () => new Promise((_, reject) => signal.addEventListener('abort', () => reject(new DOMException('Aborted', 'AbortError'))));
  const ctx = { AbortController, ApiError, API_BASE: '', navigator: { onLine: true },
    reportServerIssue: () => reports++,
    window: { setTimeout: (cb, ms) => { assert.equal(ms, 120000); expire = cb; return 1; }, clearTimeout: () => cleared = true },
    fetch: async (_, options) => {
      signal = options.signal;
      if (!stallBody) return pending();
      return { ok: true, headers: new Headers(), blob: pending };
    }, URL: { createObjectURL: () => assert.fail('timed out body cannot create download') } };
  runInNewContext(fn, ctx);
  const result = ctx.downloadFile('/download', 'file.zip');
  await new Promise(resolve => setImmediate(resolve));
  expire();
  await assert.rejects(result, e => e.code === 'TIMEOUT' && e.message.includes('다시 시도'));
  assert.ok(cleared);
  assert.equal(reports, 0, 'a slow download alone must not report global server failure');
}
(async () => {
  await scenario(false);
  await scenario(true);
  console.log('PASS: download deadline covers stalled headers and stalled body; explicit timeout and cleanup');
})().catch(e => { console.error(e); process.exitCode = 1; });
