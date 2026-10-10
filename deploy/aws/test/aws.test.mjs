// node --test deploy/aws/test/aws.test.mjs
import test from 'node:test';
import assert from 'node:assert/strict';
import { runInNewContext } from 'node:vm';
import { APPS, cacheGroups, parseHeaders } from '../apps.mjs';
import { apiFunctionCode, functionCode, template } from '../template.mjs';
import { syncCommands } from '../publish.mjs';

async function viewer(app) {
  const sandbox = {};
  runInNewContext(`${await functionCode(app)}\nthis.handler = handler;`, sandbox);
  return (uri, { host = APPS[app].aliases[0], qs = {} } = {}) => sandbox.handler({
    context: { eventType: 'viewer-request' },
    request: { uri, querystring: qs, headers: { host: { value: host } } },
  });
}

test('studio: SPA 폴백, 파일은 그대로, _redirects 이동은 쿼리 유지', async () => {
  const run = await viewer('studio');
  assert.equal(run('/').uri, '/index.html');
  assert.equal(run('/releases/42').uri, '/index.html');
  assert.equal(run('/assets/index-abc.js').uri, '/assets/index-abc.js');
  const moved = run('/studio/', { qs: { a: { value: '1' }, b: { value: '' } } });
  assert.equal(moved.statusCode, 308);
  assert.equal(moved.headers.location.value, '/?a=1&b');
  assert.equal(run('/index.html').headers.location.value, '/');
});

test('admin: 화면 경로는 index.html', async () => {
  const run = await viewer('admin');
  assert.equal(run('/reviews/1').uri, '/index.html');
  assert.equal(run('/favicon.ico').uri, '/favicon.ico');
});

test('survey: 슬래시 경로는 index.html, index.html은 슬래시로', async () => {
  const run = await viewer('survey');
  assert.equal(run('/').uri, '/index.html');
  assert.equal(run('/index.html').headers.location.value, '/');
  assert.equal(run('/assets/x.js').uri, '/assets/x.js');
});

test('landing: worker.js 와 같은 라우팅', async () => {
  const run = await viewer('landing');
  assert.equal(run('/').uri, '/index.html');
  assert.equal(run('/faq/').uri, '/faq/index.html');
  assert.equal(run('/faq').headers.location.value, '/faq/');
  assert.equal(run('/faq/index.html').headers.location.value, '/faq/');
  assert.equal(run('/robots.txt').uri, '/robots.txt');
  assert.equal(run('/assets/guide.css').uri, '/assets/guide.css');
  assert.equal(run('/nope').statusCode, 404);
  assert.equal(run('/faq', { host: 'www.audeniq.com', qs: { q: { value: 'x' } } }).headers.location.value, 'https://audeniq.com/faq/?q=x');
  assert.equal(run('/studio/login').headers.location.value, 'https://studio.audeniq.com/login');
  assert.equal(run('/', { qs: { site: { value: 'studio' } } }).headers.location.value, 'https://studio.audeniq.com/');
  const robots = run('/robots.txt', { host: 'd111.cloudfront.net' });
  assert.equal(robots.statusCode, 200);
  assert.match(robots.body.data, /Disallow: \//);
  assert.equal(run('/sitemap.xml', { host: 'd111.cloudfront.net' }).statusCode, 404);
});

test('landing viewer-response: 오류·미리보기 주소는 noindex', async () => {
  const sandbox = {};
  runInNewContext(`${await functionCode('landing')}\nthis.handler = handler;`, sandbox);
  const respond = (host, statusCode) => sandbox.handler({
    context: { eventType: 'viewer-response' },
    request: { uri: '/', querystring: {}, headers: { host: { value: host } } },
    response: { statusCode, headers: {} },
  }).headers['x-robots-tag'];
  assert.equal(respond('audeniq.com', 200), undefined);
  assert.equal(respond('audeniq.com', 404).value, 'noindex, nofollow, noarchive');
  assert.equal(respond('d111.cloudfront.net', 200).value, 'noindex, nofollow, noarchive');
});

test('API 함수: 브라우저의 x-audeniq-* 를 버리고 사용자 IP를 넣는다, 설문은 /api/survey 로', () => {
  const run = (app, uri) => {
    const sandbox = {};
    runInNewContext(`${apiFunctionCode(app)}\nthis.handler = handler;`, sandbox);
    return sandbox.handler({
      viewer: { ip: '2001:db8::7' },
      request: { uri, querystring: {}, headers: {
        'x-audeniq-service': { value: 'forged' }, 'x-audeniq-client-ip': { value: '1.1.1.1' },
        'x-audeniq-site': { value: 'admin' }, cookie: { value: 'a=b' },
      } },
    });
  };
  const r = run('studio', '/api/me');
  assert.equal(r.uri, '/api/me');
  assert.deepEqual(Object.keys(r.headers).sort(), ['cookie', 'x-audeniq-client-ip']);
  assert.equal(r.headers['x-audeniq-client-ip'].value, '2001:db8::7');
  assert.equal(run('survey', '/api/responses').uri, '/api/survey/responses');
  assert.equal(run('survey', '/api/config').uri, '/api/survey/config');
});

test('템플릿: /api 만 백엔드로(캐시 없음, 서비스 비밀·사이트 헤더), 나머지는 S3', async () => {
  for (const app of Object.keys(APPS)) {
    const t = await template(app);
    const d = t.Resources.Distribution.Properties.DistributionConfig;
    assert.equal(d.DefaultCacheBehavior.TargetOriginId, 's3');
    assert.deepEqual(d.CacheBehaviors.map(b => b.PathPattern), APPS[app].api);
    for (const b of d.CacheBehaviors) {
      assert.equal(b.TargetOriginId, 'api');
      assert.equal(b.CachePolicyId, '4135ea2d-6df8-44a3-9df3-4b5a84be39ad');
      assert.ok(b.AllowedMethods.includes('POST'));
      assert.deepEqual(b.FunctionAssociations[0].FunctionARN, { 'Fn::GetAtt': ['ApiFunction', 'FunctionARN'] });
    }
    const api = d.Origins.find(o => o.Id === 'api');
    assert.equal(!!api, APPS[app].api.length > 0);
    if (api) {
      assert.deepEqual(api.OriginCustomHeaders, [
        { HeaderName: 'x-audeniq-service', HeaderValue: { Ref: 'ServiceSecret' } },
        { HeaderName: 'x-audeniq-site', HeaderValue: APPS[app].site },
      ]);
      assert.equal(t.Parameters.ServiceSecret.NoEcho, true);
    }
    const policy = t.Resources.ResponseHeaders.Properties.ResponseHeadersPolicyConfig;
    assert.equal(policy.SecurityHeadersConfig.StrictTransportSecurity.AccessControlMaxAgeSec, 31536000);
    assert.match(policy.SecurityHeadersConfig.ContentSecurityPolicy.ContentSecurityPolicy, /frame-ancestors 'none'/);
  }
  const studio = (await template('studio')).Resources.ResponseHeaders.Properties.ResponseHeadersPolicyConfig;
  assert.match(studio.SecurityHeadersConfig.ContentSecurityPolicy.ContentSecurityPolicy, /r2\.cloudflarestorage\.com/);
  assert.ok(studio.CustomHeadersConfig.Items.some(h => h.Header === 'X-Robots-Tag'));
});

test('_headers Cache-Control → 업로드 그룹, HTML은 마지막 no-cache', () => {
  const groups = cacheGroups(parseHeaders('/*\n  X-A: b\n/\n  Cache-Control: no-cache\n/index.html\n  Cache-Control: no-cache\n/assets/*\n  Cache-Control: public, max-age=31536000, immutable\n'));
  assert.deepEqual(groups, [{ include: 'assets/*', cacheControl: 'public, max-age=31536000, immutable' }]);
  const commands = syncCommands('studio', 'bucket');
  assert.equal(commands.at(-1).at(-1), 'no-cache');
  assert.ok(commands.every(c => c.includes('*.br') && c.includes('_headers')));
});
