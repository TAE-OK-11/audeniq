// CloudFront Function (cloudfront-js-2.0, viewer-request) — STUDIO 화면 (S3)
// web/studio/public/_redirects 와 Workers의 SPA 폴백을 대신한다. /api/*, /ready 는 Worker behavior로 가서 여기를 거치지 않는다.
var REDIRECTS = { '/index.html': '/', '/studio': '/', '/studio/': '/', '/studio/index.html': '/' };

function query(qs) {
  var parts = [];
  for (var key in qs) {
    var item = qs[key];
    var values = item.multiValue ? item.multiValue : [item];
    for (var i = 0; i < values.length; i++) parts.push(values[i].value === '' ? key : key + '=' + values[i].value);
  }
  return parts.length ? '?' + parts.join('&') : '';
}

function handler(event) {
  var request = event.request;
  var to = REDIRECTS[request.uri];
  if (to) {
    return { statusCode: 308, statusDescription: 'Permanent Redirect',
      headers: { location: { value: to + query(request.querystring) }, 'cache-control': { value: 'no-store' } } };
  }
  // 확장자 없는 화면 경로는 index.html (SPA 폴백). 파일 경로는 그대로 → 없으면 S3 404
  if (!/\.[A-Za-z0-9]+$/.test(request.uri)) request.uri = '/index.html';
  return request;
}
