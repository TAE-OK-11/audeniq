// Local only: node deploy/cloudfront/verify-edge-v7.cjs /path/to/cloudfront-error-pages
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const path = require('node:path');
const workspace = process.argv[2];
if (!workspace) throw new Error('Pass the workspace directory containing v5 and v6');
function load(file) {
  const ctx = vm.createContext({});
  vm.runInContext(fs.readFileSync(file, 'utf8'), ctx);
  return ctx;
}
const old = load(path.join(workspace, 'studio-edge-function-v6.js'));
const live = load(path.join(workspace, 'studio-edge-function-v5.js'));
const candidatePath = path.join(__dirname, 'studio-edge-function-v7.js');
const next = load(candidatePath);
assert.ok(fs.statSync(candidatePath).size < 10240, 'CloudFront Function size');
const paths = ['/', '/api', '/api/releases', '/ready', '/health', '/dashboard/', '/assets/app.js',
  '/.well-known/openid-configuration', '/.well-known/nope', '/.well-known/oauth-authorization-server',
  '/.well-known/oauth-protected-resource', '/blog/wp-config.php.bak', '//2019/wp-includes/wlwmanifest.xml',
  ...Array.from(old.SCANNER_PREFIXES, p => p + '/probe'),
  ...Array.from(old.SCANNER_SUFFIXES, p => '/probe' + p), '/PhpMyAdmin/', '/WP-ADMIN'];
const agents = ['', 'Mozilla/5.0', 'Googlebot', 'SemrushBot', 'GPTBot', 'unknown crawler', 'sqlmap'];
let count = 0;
const plain = x => JSON.parse(JSON.stringify(x));
for (const uri of paths) for (const ua of agents) for (const method of ['GET', 'HEAD']) {
  const event = { viewer: { ip: '2001:db8::42' }, request: { uri, method, headers: {
    'user-agent': { value: ua }, 'x-audeniq-service': { value: 'forged' },
    'X-Audeniq-Site': { value: 'admin' }, 'x-audeniq-client-ip': { value: 'forged' },
    accept: { value: 'application/json' },
  } } };
  const results = [live, old, next].map(ctx => plain(ctx.handler(structuredClone(event))));
  for (const result of results) {
    if (result.headers['content-type']?.value.startsWith('text/html')) {
      assert.ok(result.body.includes('href="/"'));
      result.body = '<HTML design intentionally differs>';
    }
  }
  assert.deepEqual(results[2], results[1], `${method} ${uri} ${ua}: v6`);
  assert.deepEqual(results[2], results[0], `${method} ${uri} ${ua}: v5`);
  if (!results[2].statusCode) {
    assert.equal(results[2].headers['x-audeniq-client-ip'].value, event.viewer.ip);
    assert.equal(results[2].headers['x-audeniq-service'], undefined);
    assert.equal(results[2].headers['X-Audeniq-Site'], undefined);
  }
  count++;
}
for (const name of ['403.html', '404.html', '500.html', '503.html', 'error-template.html']) {
  assert.equal(fs.readFileSync(path.join(__dirname, 'errors', name), 'utf8'), fs.readFileSync(path.join(workspace, name), 'utf8'));
}
assert.equal(fs.readFileSync(candidatePath, 'utf8'), fs.readFileSync(path.join(workspace, 'studio-edge-function-v7.js'), 'utf8'));
console.log(`PASS: ${count} request cases compared against both v5 and v6; IP/header checks, size and 6 workspace copies verified`);
