// Local only. No AWS SDK/CLI or network calls.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const code = fs.readFileSync(__dirname + '/studio-maintenance-function.js', 'utf8');
assert.ok(Buffer.byteLength(code) <= 10240);
const ctx = vm.createContext({});
vm.runInContext(code, ctx);
let cases = 0;
for (const uri of ['/api/', '/api', '/ready', '/health', '/assets/app.js']) {
  for (const statusCode of [502, 503, 504, 500, 403, 404, 200]) {
    const response = { statusCode, headers: { 'content-encoding': { value: 'br' }, 'content-length': { value: '123' } },
      body: '{"error":{"code":"MAINTENANCE"},"maintenance":{"starts_at":"2026-10-28T00:00:00Z"}}' };
    const before = structuredClone(response);
    const out = ctx.handler({ request: { uri }, response });
    if (![502, 504].includes(statusCode)) {
      assert.equal(out, response);
      assert.deepEqual(out, before, `${uri} ${statusCode}: preserve origin response`);
    } else {
      assert.equal(out.statusCode, 503);
      assert.equal(out.headers['cache-control'].value, 'no-store');
      assert.equal(out.headers['content-length'], undefined);
      assert.equal(out.headers['content-encoding'], undefined);
      if (uri.startsWith('/assets')) {
        assert.match(out.headers['content-type'].value, /text\/html/);
        assert.match(out.body, /#F5F8FF/);
        assert.doesNotMatch(out.body, /https:\/\//);
      } else {
        assert.deepEqual(JSON.parse(out.body), { error: { code: 'MAINTENANCE' }, maintenance: {} });
      }
    }
    cases++;
  }
}
console.log(`PASS: ${cases} path/status cases; maintenance window preservation, headers, design, size`);
