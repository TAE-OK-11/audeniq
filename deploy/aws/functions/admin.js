// CloudFront Function (cloudfront-js-2.0, viewer-request) — ADMIN 화면 (S3)
// Workers의 SPA 폴백을 대신한다. /api/* 는 백엔드 behavior(functions/edge.js)로 가서 여기를 거치지 않는다.
function handler(event) {
  var request = event.request;
  if (!/\.[A-Za-z0-9]+$/.test(request.uri)) request.uri = '/index.html';
  return request;
}
