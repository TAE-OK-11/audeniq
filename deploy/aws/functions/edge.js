// CloudFront Function (cloudfront-js-2.0, viewer-request + viewer-response) — 백엔드(api, 443)로 가는 요청
// Cloudflare 프록시 Worker(web/studio/worker.ts, web/admin/worker.ts)가 하던 일을 대신한다.
// 요청 헤더 규칙은 web/shared/transport.ts backendHeaders 와 같다 (deploy/aws/test/aws.test.ts가 맞춰 본다):
// - 필요한 브라우저 헤더만 남기고 나머지는 지운다. 브라우저가 보낸 x-audeniq-* 는 늘 지운다
//   (서비스 비밀 x-audeniq-service·화면 이름 x-audeniq-site는 CloudFront origin custom header가 붙인다)
// - 사용자 IP(event.viewer.ip)를 x-audeniq-client-ip로 넣는다 (로그인 시도 제한, 스태프 접근 기록)
// 쿠키·쿼리는 헤더가 아니라 따로 오고(origin request policy가 전달), Host·Accept-Encoding·Content-Length는
// CloudFront가 다룬다. 응답(viewer-response)은 /api/ 의 CORS·캐시 헤더만 Worker와 같게 맞춘다.
// 아래 두 값은 배포 템플릿이 앱마다 채운다 (deploy/aws/template.ts).
var CORS_ORIGIN = '';
var API_NO_STORE = false;

var FORWARD = ['origin', 'content-type', 'accept', 'x-csrf-token', 'sec-fetch-site', 'x-request-id', 'user-agent'];
// 백엔드가 서빙하는 화면 파일은 조건부 요청으로 304를 받는다
var STATIC = ['if-none-match', 'if-modified-since'];
// AI 어시스턴트(MCP)의 서버 간 호출: 이 경로에만 Authorization과 MCP 메타데이터
var MCP_PATHS = ['/api/mcp', '/api/mcp/', '/api/mcp/public', '/api/oauth/token', '/api/oauth/register', '/api/oauth/revoke', '/api/oauth/device_authorization'];
var MCP = ['authorization', 'mcp-protocol-version', 'mcp-method', 'mcp-name'];
var PARTNER_HOOK = '/api/partner-hooks/';
// CloudFront 앞에서는 cf-connecting-ip도 브라우저가 지어낼 수 있는 값이다
var PARTNER_DROP = ['x-forwarded-for', 'x-real-ip', 'cf-connecting-ip'];
// CloudFront가 관리하는 헤더는 그대로 둔다
var CLOUDFRONT = ['host', 'accept-encoding', 'content-length'];

function has(list, value) {
  return list.indexOf(value) !== -1;
}

function onRequest(event) {
  var request = event.request;
  var headers = request.headers;
  var path = request.uri;
  var api = path.indexOf('/api/') === 0;
  var mcp = has(MCP_PATHS, path);
  var partner = path.indexOf(PARTNER_HOOK) === 0;
  for (var name in headers) {
    var keep = name.indexOf('x-audeniq-') !== 0 && (
      has(CLOUDFRONT, name) || has(FORWARD, name) || (!api && has(STATIC, name)) || (mcp && has(MCP, name))
      || (partner && !has(PARTNER_DROP, name)));
    if (!keep) delete headers[name];
  }
  headers['x-audeniq-client-ip'] = { value: event.viewer.ip };
  return request;
}

function onResponse(event) {
  var response = event.response;
  if (event.request.uri.indexOf('/api/') !== 0) return response;
  var headers = response.headers;
  if (CORS_ORIGIN) {
    headers['access-control-allow-origin'] = { value: CORS_ORIGIN };
    headers['access-control-allow-credentials'] = { value: 'true' };
  } else {
    delete headers['access-control-allow-origin'];
    delete headers['access-control-allow-credentials'];
  }
  if (API_NO_STORE) {
    headers['cache-control'] = { value: 'no-store' };
    headers['x-content-type-options'] = { value: 'nosniff' };
  }
  return response;
}

function handler(event) {
  return event.context.eventType === 'viewer-response' ? onResponse(event) : onRequest(event);
}
