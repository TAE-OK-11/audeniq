// AUDENIQ Studio maintenance page (viewer-response)
// 오리진이 502/503/504를 내면(업데이트·서버 다운) 기본 에러 페이지 대신
// AUDENIQ 점검 화면을 낸다. /api/*는 JSON으로 응답한다.
// 500·403 등 다른 상태는 그대로 둔다 (백엔드·경계가 처리).
// 외부 리소스를 쓰지 않는다 (오리진이 죽었을 때는 정적 파일도 못 가져오므로 전부 인라인).

function handler(event) {
  var response = event.response;
  var status = response.statusCode;
  if (status !== 502 && status !== 503 && status !== 504) {
    return response;
  }

  var uri = event.request.uri || '/';
  response.statusCode = 503;
  response.statusDescription = 'Service Unavailable';
  response.headers['retry-after'] = { value: '60' };

  if (uri.indexOf('/api/') === 0) {
    response.headers['content-type'] = { value: 'application/json; charset=utf-8' };
    response.body = '{"error":"maintenance","message":"AUDENIQ Studio is temporarily unavailable. Please try again shortly."}';
    return response;
  }

  response.headers['content-type'] = { value: 'text/html; charset=utf-8' };
  response.body =
    '<!DOCTYPE html><html lang="ko"><head><meta charset="utf-8">' +
    '<meta name="viewport" content="width=device-width,initial-scale=1">' +
    '<title>AUDENIQ STUDIO - 잠시 점검 중입니다</title>' +
    '<style>body{margin:0;font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,"Helvetica Neue",Arial,sans-serif;background:#0b0b0f;color:#f5f5f7;display:flex;min-height:100vh;align-items:center;justify-content:center}main{text-align:center;padding:32px}h1{font-size:28px;letter-spacing:.04em;margin:0 0 12px}p{color:#a1a1aa;margin:0 0 8px;line-height:1.6}small{color:#71717a}</style>' +
    '</head><body><main><h1>AUDENIQ STUDIO</h1>' +
    '<p>지금 서버를 업데이트하거나 점검하고 있어요.</p>' +
    '<p>잠시 후 다시 시도해 주세요.</p>' +
    '<small>이 페이지는 서버가 복구되면 사라집니다.</small>' +
    '</main></body></html>';
  return response;
}
