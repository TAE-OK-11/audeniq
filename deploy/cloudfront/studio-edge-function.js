// AUDENIQ Studio CloudFront Function (viewer-request)
// Worker(audeniq-studio-proxy)가 하던 엣지 일 중 CloudFront Function이 맡는 부분.
// 나머지(SPA 폴백·리다이렉트·캐시·보안 헤더·well-known)는 백엔드(studio_static.rs)가 한다.
//
// 하는 일 (2가지뿐):
//  1. 브라우저가 보낸 x-audeniq-* 헤더를 전부 버린다.
//     (백엔드는 x-audeniq-service가 맞으면 요청을 엣지로 신뢰하므로,
//      브라우저가 x-audeniq-client-ip를 위조해 보내는 것을 막아야 한다)
//  2. 실제 시청자 IP를 x-audeniq-client-ip로 넣는다.
//     (Worker의 CF-Connecting-IP -> x-audeniq-client-ip 와 동일)
// x-audeniq-service / x-audeniq-site는 오리진 사용자 정의 헤더(정적)로
// CloudFront가 오리진 요청 시 붙이므로 이 함수에서 다루지 않는다.

function handler(event) {
  var request = event.request;
  var headers = request.headers;

  // 1. 브라우저발 x-audeniq-* 제거
  Object.keys(headers).forEach(function (name) {
    if (name.toLowerCase().indexOf('x-audeniq-') === 0) {
      delete headers[name];
    }
  });

  // 2. 실제 클라이언트 IP 주입
  headers['x-audeniq-client-ip'] = { value: event.viewer.ip };

  return request;
}
