// Agent draft using the v9 error-page design; not a final user-approved design.
// Origin 400+ viewer-response execution is NOT established. Independent error
// origin + CustomErrorResponses remain an infrastructure decision (not wired here).
// CloudFront Functions cannot inspect the origin response body. Preserve every
// 503, including backend MAINTENANCE JSON and its maintenance window, verbatim.
// Keep 500 (application errors), 403 and 404 intact; map gateway failures only.
function handler(event) {
  var response = event.response;
  if (response.statusCode !== 502 && response.statusCode !== 504) return response;
  var uri = event.request.uri || '/';
  response.statusCode = 503;
  response.statusDescription = 'Service Unavailable';
  response.headers['cache-control'] = { value: 'no-store' };
  response.headers['retry-after'] = { value: '60' };
  response.headers['x-content-type-options'] = { value: 'nosniff' };
  response.headers['x-frame-options'] = { value: 'DENY' };
  response.headers['referrer-policy'] = { value: 'no-referrer' };
  // Replacement bodies no longer carry the origin entity's metadata.
  delete response.headers['content-length'];
  delete response.headers['content-encoding'];
  delete response.headers['etag'];
  if (uri === '/api' || uri.indexOf('/api/') === 0 || uri === '/ready' || uri === '/health') {
    response.headers['content-type'] = { value: 'application/json; charset=utf-8' };
    response.body = '{"error":{"code":"MAINTENANCE"},"maintenance":{}}';
  } else {
    response.headers['content-type'] = { value: 'text/html; charset=utf-8' };
    response.body = "<!DOCTYPE html>\n<html lang=\"ko\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n<meta http-equiv=\"refresh\" content=\"60\"><title>AUDENIQ STUDIO - 잠시 점검 중입니다</title>\n<style>*{box-sizing:border-box}body{margin:0;font-family:\"Audeniq Sans\",\"Audeniq Sans Full\",-apple-system,BlinkMacSystemFont,\"Apple SD Gothic Neo\",\"Noto Sans KR\",\"Malgun Gothic\",sans-serif;background:#fff;color:#171B29;min-height:100vh;min-height:100dvh;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:22px;padding:32px 16px;word-break:keep-all;overflow-wrap:anywhere}.brand{display:inline-flex;align-items:center;gap:10px;color:#171B29;text-decoration:none;font-size:22px;font-weight:800;letter-spacing:-.02em}.brand span{font-size:11px;letter-spacing:.14em;color:#3B63F3;background:#F5F8FF;border-radius:8px;padding:6px 10px}.brand span.aq-logo{background:transparent;color:#171B29;font-size:22px;font-weight:800;letter-spacing:0;padding:0;border-radius:0;font-family:\"Audeniq Sans\",\"Audeniq Sans Full\",-apple-system,BlinkMacSystemFont,\"Apple SD Gothic Neo\",\"Noto Sans KR\",\"Malgun Gothic\",sans-serif;font-feature-settings:\"ss20\";}main{width:100%;max-width:520px;margin-bottom:6vh;padding:40px 22px 30px;border-radius:28px;background:#F5F8FF;text-align:center}.icon{display:grid;place-items:center;width:60px;height:60px;border-radius:50%;margin:0 auto 18px;background:#fff;color:#3B63F3;box-shadow:0 0 0 8px #3b63f30f}.code{display:table;margin:0 auto 10px;padding:2px 9px;border-radius:32px;background:#fff;color:#3B63F3;font-size:12px;font-weight:800;letter-spacing:.02em}h1{margin:0;font-size:20px;font-weight:700;line-height:1.45;letter-spacing:-.02em}p{margin:8px auto 0;max-width:420px;color:#646E84;font-size:14px;line-height:1.7}.actions{margin-top:26px}a.btn{display:inline-block;min-width:120px;padding:12px 20px;border-radius:14px;background:#3B63F3;color:#fff;text-decoration:none;font-size:14px;font-weight:700}a.btn:hover{background:#2C56E0}a.sub{display:block;margin-top:16px;color:#4D5970;font-size:14px;text-decoration:none}a:focus-visible{outline:3px solid #6F91FF;outline-offset:4px}@media(min-width:768px){main{padding:56px 32px 40px}h1{font-size:23px}p{font-size:15px}}</style>\n</head>\n<body>\n<a class=\"brand\" href=\"/\" aria-label=\"AUDENIQ STUDIO 홈\"><span class=\"aq-logo\">AUDENIQ</span><span>STUDIO</span></a>\n<main>\n<span class=\"icon\" aria-hidden=\"true\"><svg viewBox=\"0 0 24 24\" width=\"28\" height=\"28\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.8\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><path d=\"M14.7 6.3a4 4 0 0 0-5.4 5.4L3 18v3h3l6.3-6.3a4 4 0 0 0 5.4-5.4l-2.6 2.6-2.4-.6-.6-2.4Z\"/></svg></span>\n<p class=\"code\">503</p>\n<h1>잠시 점검 중입니다</h1>\n<p>지금 서버를 업데이트하거나 점검하고 있어요. 잠시 후 다시 시도해 주세요.</p>\n<div class=\"actions\"><a class=\"btn\" href=\"/\">홈으로 돌아가기</a>\n<a class=\"sub\" href=\"javascript:history.back()\">이전 페이지로 돌아가기</a>\n</div>\n</main>\n</body>\n</html>";
  }
  return response;
}
