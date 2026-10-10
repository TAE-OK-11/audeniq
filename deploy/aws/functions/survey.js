// CloudFront Function (cloudfront-js-2.0, viewer-request) — 설문 화면 (S3)
// Workers의 html_handling: auto-trailing-slash 를 대신한다. 없는 파일은 CloudFront 오류 응답(404.html)이 처리한다.
function handler(event) {
  var request = event.request;
  var uri = request.uri;
  if (uri === '/index.html' || /\/index\.html$/.test(uri)) {
    return { statusCode: 308, statusDescription: 'Permanent Redirect',
      headers: { location: { value: uri.slice(0, -'index.html'.length) }, 'cache-control': { value: 'no-store' } } };
  }
  if (uri.charAt(uri.length - 1) === '/') request.uri = uri + 'index.html';
  return request;
}
