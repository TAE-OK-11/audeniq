// CloudFront Function (cloudfront-js-2.0, viewer-request) — /api/* → 백엔드(ALB)
// 예전 Worker가 하던 요청 정리를 대신한다:
// - 브라우저가 보낸 x-audeniq-* 헤더는 모두 버린다 (서비스 비밀·사이트는 CloudFront origin custom header가 붙인다)
// - 로그인 시도 제한용 사용자 IP를 x-audeniq-client-ip로 넣는다 (crate::auth::CLIENT_IP_HEADER)
// - 앱 설정에 apiPrefix가 있으면 /api/x 를 그 접두사 아래로 바꾼다 (배포 템플릿이 REWRITE_PREFIX를 채운다)
var REWRITE_PREFIX = '';

function handler(event) {
  var request = event.request;
  var headers = request.headers;
  for (var name in headers) {
    if (name.indexOf('x-audeniq-') === 0) delete headers[name];
  }
  headers['x-audeniq-client-ip'] = { value: event.viewer.ip };
  if (REWRITE_PREFIX && request.uri.indexOf('/api/') === 0) request.uri = REWRITE_PREFIX + request.uri.slice(4);
  return request;
}
