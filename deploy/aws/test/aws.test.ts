// node --experimental-strip-types --test deploy/aws/test/aws.test.ts
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import { APPS, cacheGroups, isAppName, parseHeaders } from '../apps.ts';
import type { AppName } from '../apps.ts';
import { edgeFunctionCode, functionCode, ORIGIN_HTTPS_PORT, template } from '../template.ts';
import type { ApiOrigin } from '../template.ts';
import { backendHeaders, MCP_DISCOVERY_PATHS } from '../../../web/shared/transport.ts';
import { syncCommands } from '../publish.ts';

// CloudFront Functions(cloudfront-js-2.0) 이벤트·결과 중 테스트가 쓰는 만큼
interface CfValue { value: string }
interface CfEvent {
  context?: { eventType: 'viewer-request' | 'viewer-response' };
  viewer?: { ip: string };
  request: { uri: string; querystring: Record<string, CfValue>; headers: Record<string, CfValue> };
  response?: { statusCode: number; headers: Record<string, CfValue> };
}
/** viewer-request 결과 요청 */
interface CfRequest {
  uri: string;
  headers: Record<string, CfValue>;
}
/** 요청(uri)이나 응답(statusCode·body)을 돌려준다 */
interface CfResult {
  uri?: string;
  statusCode?: number;
  headers: Record<string, CfValue>;
  body?: { data: string };
}
/** 함수 코드 뒤에 붙인 `this.handler = handler;` 가 채운다 */
interface Sandbox { handler?: (event: CfEvent) => CfResult }

async function viewer(app: AppName) {
  const sandbox: Sandbox = {};
  runInNewContext(`${await functionCode(app)}\nthis.handler = handler;`, sandbox);
  return (uri: string, { host = APPS[app].aliases[0], qs = {} }: { host?: string; qs?: Record<string, CfValue> } = {}) => sandbox.handler!({
    context: { eventType: 'viewer-request' },
    request: { uri, querystring: qs, headers: { host: { value: host } } },
  });
}

test('admin: 화면 경로는 index.html', async () => {
  const run = await viewer('admin');
  assert.equal(run('/reviews/1').uri, '/index.html');
  assert.equal(run('/favicon.ico').uri, '/favicon.ico');
});

test('landing: worker.ts 와 같은 라우팅', async () => {
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
  assert.match(robots.body?.data ?? '', /Disallow: \//);
  assert.equal(run('/sitemap.xml', { host: 'd111.cloudfront.net' }).statusCode, 404);
});

test('landing viewer-response: 오류·미리보기 주소는 noindex', async () => {
  const sandbox: Sandbox = {};
  runInNewContext(`${await functionCode('landing')}\nthis.handler = handler;`, sandbox);
  const respond = (host: string, statusCode: number) => sandbox.handler!({
    context: { eventType: 'viewer-response' },
    request: { uri: '/', querystring: {}, headers: { host: { value: host } } },
    response: { statusCode, headers: {} },
  }).headers['x-robots-tag'];
  assert.equal(respond('audeniq.com', 200), undefined);
  assert.equal(respond('audeniq.com', 404).value, 'noindex, nofollow, noarchive');
  assert.equal(respond('d111.cloudfront.net', 200).value, 'noindex, nofollow, noarchive');
});

function edge(app: AppName) {
  const sandbox: { handler?: (event: unknown) => unknown } = {};
  runInNewContext(`${edgeFunctionCode(app)}\nthis.handler = handler;`, sandbox);
  return {
    request: (uri: string, headers: Record<string, string>, ip = '2001:db8::7') => sandbox.handler!({
      context: { eventType: 'viewer-request' }, viewer: { ip },
      request: { uri, querystring: {}, headers: Object.fromEntries(Object.entries(headers).map(([k, v]) => [k, { value: v }])) },
    }) as CfRequest,
    response: (uri: string, headers: Record<string, string>) => (sandbox.handler!({
      context: { eventType: 'viewer-response' },
      request: { uri, querystring: {}, headers: {} },
      response: { statusCode: 200, headers: Object.fromEntries(Object.entries(headers).map(([k, v]) => [k, { value: v }])) },
    }) as { headers: Record<string, CfValue> }).headers,
  };
}

// 브라우저·공격자가 보낼 수 있는 헤더 (CloudFront Functions에서는 쿠키가 headers 밖에 따로 온다)
const BROWSER = {
  host: 'studio.audeniq.com', 'accept-encoding': 'br, gzip', 'content-length': '2',
  origin: 'https://studio.audeniq.com', 'content-type': 'application/json', accept: '*/*', 'x-csrf-token': 't',
  'sec-fetch-site': 'same-origin', 'x-request-id': 'r1', 'user-agent': 'UA', 'if-none-match': '"e"', 'if-modified-since': 'x',
  authorization: 'Bearer leak', 'mcp-protocol-version': '2025-06-18', 'mcp-method': 'tools/call', 'mcp-name': 'n',
  'x-audeniq-service': 'forged', 'x-audeniq-client-ip': '1.1.1.1', 'x-audeniq-site': 'admin', 'x-audeniq-audit-route': 'x',
  'x-forwarded-for': '9.9.9.9', 'x-real-ip': '9.9.9.9', 'x-partner-signature': 'sig', 'cf-connecting-ip': '8.8.8.8', referer: 'r',
};

test('legacy edge helper: 프록시 Worker(backendHeaders)와 같은 헤더만 백엔드로, 사용자 IP는 CloudFront 값', () => {
  const run = edge('studio');
  const paths = ['/', '/releases/1', '/assets/a.js', '/api/me', '/api/auth/login', '/api/mcp', '/api/oauth/token',
    '/api/partner-hooks/acme/status', ...MCP_DISCOVERY_PATHS, '/ready'];
  for (const path of paths) {
    const out = run.request(path, BROWSER);
    assert.equal(out.uri, path, '경로는 바꾸지 않는다 (SPA 폴백·리다이렉트는 백엔드)');
    assert.equal(out.headers['x-audeniq-client-ip'].value, '2001:db8::7');
    const sent = Object.keys(out.headers).filter(h => !['host', 'accept-encoding', 'content-length', 'x-audeniq-client-ip'].includes(h)).sort();
    // Worker 규칙: cf-connecting-ip → x-audeniq-client-ip, 서비스 비밀·사이트는 CloudFront origin custom header
    const request = new Request(`https://studio.audeniq.com${path}`, { headers: BROWSER });
    const worker = [...backendHeaders(request, { EDGE_SERVICE_SECRET: 's' }, path, 'studio').keys()]
      .filter(h => !['accept-encoding', 'x-audeniq-client-ip', 'x-audeniq-service', 'x-audeniq-site', 'content-length', 'cf-connecting-ip'].includes(h)).sort();
    assert.deepEqual(sent, worker, path);
    assert.ok(sent.every(h => !h.startsWith('x-audeniq-')), path);
  }
  assert.equal(run.request('/api/me', BROWSER).headers.authorization, undefined, '베어러 토큰은 MCP 경로에만');
  assert.equal(run.request('/api/mcp', BROWSER).headers.authorization.value, 'Bearer leak');
});

test('legacy edge helper 응답: studio CORS 옵션, admin no-store (Studio template uses v9)', () => {
  const studio = edge('studio').response('/api/me', { 'access-control-allow-origin': '*' });
  assert.equal(studio['access-control-allow-origin'].value, 'https://studio.audeniq.com');
  assert.equal(studio['access-control-allow-credentials'].value, 'true');
  assert.equal(edge('studio').response('/', {})['access-control-allow-origin'], undefined, '화면 파일에는 붙이지 않는다');
  const admin = edge('admin').response('/api/me', { 'access-control-allow-origin': '*', 'cache-control': 'max-age=30' });
  assert.equal(admin['access-control-allow-origin'], undefined);
  assert.equal(admin['cache-control'].value, 'no-store');
  assert.equal(admin['x-content-type-options'].value, 'nosniff');
});

test('템플릿 studio: S3 없이 화면·API 모두 백엔드(443), 해시 에셋만 캐시', async () => {
  const t = await template('studio');
  assert.equal(t.Resources.Bucket, undefined);
  assert.equal(t.Resources.ViewerFunction, undefined);
  assert.equal(t.Outputs.BucketName, undefined);
  const d = t.Resources.Distribution.Properties.DistributionConfig;
  assert.deepEqual(d.Origins.map(o => o.Id), ['api']);
  const api = d.Origins[0] as ApiOrigin;
  assert.equal(api.CustomOriginConfig.HTTPSPort, ORIGIN_HTTPS_PORT);
  assert.equal(api.CustomOriginConfig.OriginProtocolPolicy, 'https-only');
  assert.deepEqual(api.OriginCustomHeaders, [
    { HeaderName: 'x-audeniq-service', HeaderValue: { Ref: 'ServiceSecret' } },
    { HeaderName: 'x-audeniq-site', HeaderValue: 'studio' },
  ]);
  const def = d.DefaultCacheBehavior;
  assert.equal(def.TargetOriginId, 'api');
  assert.equal(def.CachePolicyId, '4135ea2d-6df8-44a3-9df3-4b5a84be39ad');
  assert.ok(def.AllowedMethods.includes('POST'));
  assert.deepEqual(def.FunctionAssociations.map(f => f.EventType), ['viewer-request', 'viewer-response']);
  assert.deepEqual(d.CacheBehaviors.map(b => [b.PathPattern, b.CachePolicyId, b.OriginRequestPolicyId]),
    [
      ...['/api', '/api/*', '/.well-known/*'].map(path => [path,
        '4135ea2d-6df8-44a3-9df3-4b5a84be39ad', 'b689b0a8-53d0-40ab-baf2-68738e2966ac']),
      ...(APPS.studio.origin === 'backend' ? APPS.studio.cached : []).map(path => [path, { Ref: 'AssetCache' }, undefined]),
    ]);
  const cache = t.Resources.AssetCache!.Properties.CachePolicyConfig;
  assert.equal(cache.MinTTL, 0);
  assert.equal(cache.DefaultTTL, 0);
  assert.equal(cache.MaxTTL, 31536000);
  const key = cache.ParametersInCacheKeyAndForwardedToOrigin as Record<string, unknown>;
  assert.deepEqual(key.HeadersConfig, { HeaderBehavior: 'whitelist', Headers: ['Accept'] });
  assert.equal(key.EnableAcceptEncodingBrotli, true);
  assert.equal(key.EnableAcceptEncodingGzip, true);
  assert.deepEqual(key.CookiesConfig, { CookieBehavior: 'none' });
  assert.deepEqual(key.QueryStringsConfig, { QueryStringBehavior: 'none' });
  // Simulate first-match behavior, including nested API files with image suffixes.
  for (const path of ['/api/a.png', '/api/orgs/o/assets/a.jpg', '/.well-known/logo.avif']) {
    const behavior = d.CacheBehaviors.find(b => new RegExp('^' + b.PathPattern.replace(/[.]/g, '\\.').replace(/\*/g, '.*') + '$').test(path));
    assert.equal(behavior?.CachePolicyId, '4135ea2d-6df8-44a3-9df3-4b5a84be39ad', path);
  }
  const policy = t.Resources.ResponseHeaders.Properties.ResponseHeadersPolicyConfig;
  assert.equal(policy.SecurityHeadersConfig.StrictTransportSecurity.AccessControlMaxAgeSec, 31536000);
  assert.equal(policy.SecurityHeadersConfig.ContentSecurityPolicy, undefined, 'CSP는 백엔드(_headers)가 붙인다');
  assert.equal(t.Parameters.ServiceSecret?.NoEcho, true);
});

test('템플릿 admin·landing: 화면은 S3, /api 만 백엔드로(캐시 없음, edge 함수)', async () => {
  for (const app of (['admin', 'landing'] as const)) {
    const conf = APPS[app];
    assert.equal(conf.origin, 's3');
    if (conf.origin !== 's3') continue;
    const t = await template(app);
    const d = t.Resources.Distribution.Properties.DistributionConfig;
    assert.equal(d.DefaultCacheBehavior.TargetOriginId, 's3');
    assert.deepEqual(d.CacheBehaviors.map(b => b.PathPattern), conf.api);
    for (const b of d.CacheBehaviors) {
      assert.equal(b.TargetOriginId, 'api');
      assert.equal(b.CachePolicyId, '4135ea2d-6df8-44a3-9df3-4b5a84be39ad');
      assert.ok(b.AllowedMethods.includes('POST'));
      assert.deepEqual(b.FunctionAssociations[0].FunctionARN, { 'Fn::GetAtt': ['EdgeFunction', 'FunctionARN'] });
    }
    const api = d.Origins.find((o): o is ApiOrigin => o.Id === 'api');
    assert.equal(!!api, conf.api.length > 0);
    assert.equal(!!t.Resources.EdgeFunction, conf.api.length > 0);
    if (api) assert.deepEqual(api.OriginCustomHeaders[1], { HeaderName: 'x-audeniq-site', HeaderValue: conf.site });
    const policy = t.Resources.ResponseHeaders.Properties.ResponseHeadersPolicyConfig;
    assert.equal(policy.SecurityHeadersConfig.StrictTransportSecurity.AccessControlMaxAgeSec, 31536000);
    assert.match(policy.SecurityHeadersConfig.ContentSecurityPolicy?.ContentSecurityPolicy ?? '', /frame-ancestors 'none'/);
  }
  assert.ok(Object.keys(APPS).every(isAppName));
});

test('_headers Cache-Control → 업로드 그룹, HTML은 마지막 no-cache', () => {
  const groups = cacheGroups(parseHeaders('/*\n  X-A: b\n/\n  Cache-Control: no-cache\n/index.html\n  Cache-Control: no-cache\n/assets/*\n  Cache-Control: public, max-age=31536000, immutable\n'));
  assert.deepEqual(groups, [{ include: 'assets/*', cacheControl: 'public, max-age=31536000, immutable' }]);
  const commands = syncCommands('admin', 'bucket');
  assert.equal(commands.at(-1)?.at(-1), 'no-cache');
  assert.ok(commands.every(c => c.includes('*.br') && c.includes('_headers')));
  assert.throws(() => syncCommands('studio', 'bucket'), /백엔드/);
});


test('Studio template uses checked-in functions; unknown cached associations are preserved', async () => {
  const t = await template('studio');
  assert.equal(t.Resources.EdgeFunction!.Properties.FunctionCode,
    readFileSync(new URL('../../cloudfront/studio-edge-function-v9.js', import.meta.url), 'utf8'));
  assert.equal(t.Resources.MaintenanceFunction!.Properties.FunctionCode,
    readFileSync(new URL('../../cloudfront/studio-maintenance-function.js', import.meta.url), 'utf8'));
  const d = t.Resources.Distribution.Properties.DistributionConfig;
  for (const b of [d.DefaultCacheBehavior, ...d.CacheBehaviors]) {
    if (typeof b.CachePolicyId !== 'string') {
      assert.deepEqual(b.FunctionAssociations, [], 'Live cached connections await inventory');
      continue;
    }
    assert.deepEqual(b.FunctionAssociations, [
      { EventType: 'viewer-request', FunctionARN: { 'Fn::GetAtt': ['EdgeFunction', 'FunctionARN'] } },
      { EventType: 'viewer-response', FunctionARN: { 'Fn::GetAtt': ['MaintenanceFunction', 'FunctionARN'] } },
    ]);
  }
  assert.equal(d.CustomErrorResponses, undefined, 'Independent error origin is an undecided infrastructure change');
});
