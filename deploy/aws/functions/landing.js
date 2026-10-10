// CloudFront Function (cloudfront-js-2.0, viewer-request + viewer-response) — 랜딩 (audeniq.com)
// web/landing/src/worker.js 의 라우팅을 대신한다. PAGE_PATHS·FILE_PATHS는 배포 템플릿을 만들 때
// web/landing/src/public-routes.js(scripts/build-seo.mjs 생성)에서 아래 자리에 채워 넣는다.
/*ROUTES*/
var PRIMARY_HOST = 'audeniq.com';
var STUDIO_ORIGIN = 'https://studio.audeniq.com';

function query(qs, skip) {
  var parts = [];
  for (var key in qs) {
    if (key === skip) continue;
    var item = qs[key];
    var values = item.multiValue ? item.multiValue : [item];
    for (var i = 0; i < values.length; i++) parts.push(values[i].value === '' ? key : key + '=' + values[i].value);
  }
  return parts.length ? '?' + parts.join('&') : '';
}

function redirect(location) {
  return { statusCode: 308, statusDescription: 'Permanent Redirect',
    headers: { location: { value: location }, 'cache-control': { value: 'no-store' } } };
}

function plain(status, body, type) {
  return { statusCode: status, statusDescription: status === 200 ? 'OK' : 'Not Found',
    headers: { 'content-type': { value: type || 'text/plain; charset=utf-8' }, 'cache-control': { value: 'no-cache' },
      'x-content-type-options': { value: 'nosniff' }, 'x-robots-tag': { value: 'noindex, nofollow, noarchive' } },
    body: { encoding: 'text', data: body } };
}

function canonicalPagePath(path) {
  if (path === '/index.html') return '/';
  for (var i = 0; i < PAGE_PATHS.length; i++) {
    var page = PAGE_PATHS[i];
    if (page === '/') continue;
    if (path === page.slice(0, -1) || path === page + 'index.html') return page;
  }
  return path;
}

function onRequest(request) {
  var host = (request.headers.host ? request.headers.host.value : '').toLowerCase().replace(/\.$/, '');
  var path = request.uri;
  var qs = request.querystring;

  // 예전 STUDIO 진입점(/studio 경로, ?site=studio)은 React STUDIO로
  var m = /^\/studio(\/.*)?$/.exec(path);
  if (m) return redirect(STUDIO_ORIGIN + (m[1] && m[1] !== '/index.html' ? m[1] : '/') + query(qs));
  if (qs.site && qs.site.value === 'studio') return redirect(STUDIO_ORIGIN + '/');

  // 검색에는 apex 하나만 — www는 경로·쿼리를 유지해 이동
  if (host === 'www.' + PRIMARY_HOST) return redirect('https://' + PRIMARY_HOST + canonicalPagePath(path) + query(qs));

  var canonical = host === PRIMARY_HOST;
  if (path === '/robots.txt' && !canonical) return plain(200, 'User-agent: *\nDisallow: /\n');
  if (path === '/sitemap.xml' && !canonical) return plain(404, 'Not found');
  if (path === '/health') return plain(200, JSON.stringify({ ok: true, service: 'AUDENIQ Web', host: host }), 'application/json');

  var canonicalPath = canonicalPagePath(path);
  if (canonicalPath !== path) return redirect(canonicalPath + query(qs));

  // 게시된 문서만 HTML로 연결하고, 모르는 경로는 진짜 404
  if (path.indexOf('/assets/') === 0 || FILE_PATHS.indexOf(path) !== -1) return request;
  if (PAGE_PATHS.indexOf(path) !== -1) { request.uri = path + 'index.html'; return request; }
  return plain(404, 'Not found');
}

function onResponse(event) {
  var response = event.response;
  var host = (event.request.headers.host ? event.request.headers.host.value : '').toLowerCase().replace(/\.$/, '');
  // 미리보기 주소(*.cloudfront.net)·오류 응답은 검색에 남기지 않는다
  if (host !== PRIMARY_HOST || response.statusCode >= 400) response.headers['x-robots-tag'] = { value: 'noindex, nofollow, noarchive' };
  return response;
}

function handler(event) {
  return event.context.eventType === 'viewer-response' ? onResponse(event) : onRequest(event.request);
}
