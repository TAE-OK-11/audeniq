// AUDENIQ Studio CloudFront Function (viewer-request) - edge v2, live 2026-10-28.
// Changes vs main (PR #342):
//  - scanner 404: plain 'Not Found' -> designed 404.html inline (no origin needed)
//  - /.well-known/* except oauth-: JSON 404 at edge (same as backend, no origin needed)
//  - header strip + client IP injection unchanged. Maintenance function untouched.
// AUDENIQ Studio CloudFront Function (viewer-request)
// Worker(audeniq-studio-proxy)가 하던 엣지 일 중 CloudFront Function이 맡는 부분.
// 나머지(SPA 폴백·리다이렉트·캐시·보안 헤더)는 백엔드(studio_static.rs)가 한다.
// well-known은 oauth- 발견 문서만 백엔드로 가고, 나머지는 아래 0b에서 JSON 404를 낸다.
//
// 하는 일:
//  0b. /.well-known/* (oauth- 제외)를 JSON 404로 돌려보낸다 (아래 참조).
//  0. 스캐너 탐색 경로를 오리진까지 보내지 않고 404로 돌려보낸다.
//     (WAF 커스텀 규칙의 URI 검사는 Free 플랜에서 막혀 있어 Function에서 처리한다.
//      Studio 정상 경로(/, /assets/*, /api/*, /ready, /.well-known/*)와는 겹치지 않는다.)
//  1. 브라우저가 보낸 x-audeniq-* 헤더를 전부 버린다.
//     (백엔드는 x-audeniq-service가 맞으면 요청을 엣지로 신뢰하므로,
//      브라우저가 x-audeniq-client-ip를 위조해 보내는 것을 막아야 한다)
//  2. 실제 시청자 IP를 x-audeniq-client-ip로 넣는다.
//     (Worker의 CF-Connecting-IP -> x-audeniq-client-ip 와 동일)
// x-audeniq-service / x-audeniq-site는 오리진 사용자 정의 헤더(정적)로
// CloudFront가 오리진 요청 시 붙이므로 이 함수에서 다루지 않는다.

// 스캐너가 자주 두드리는 경로 접두사. Studio 정상 경로와 겹치지 않는다.
var SCANNER_PREFIXES = [
  '/.env',
  '/.git',
  '/.svn',
  '/wp-admin',
  '/wp-login.php',
  '/phpmyadmin',
  '/.aws/',
  '/config.php',
  '/backup'
];

function handler(event) {
  var request = event.request;
  var uri = request.uri || '/';

  // 0b. /.well-known/* (oauth- 발견 문서 제외) -> JSON 404, 오리진 미도달.
  // 백엔드 정적 서빙과 같은 응답({"error":{"code":"NOT_FOUND","message":"NOT_FOUND"}})이다.
  // 주의: 나중에 /.well-known/에 실제 파일을 추가하면, 그 경로는 이 규칙에서 빼야 한다.
  // 오리진이 죽어도 이 경로는 AWS 504가 아니라 JSON 404가 된다.
  if (uri.indexOf('/.well-known/') === 0 && uri.indexOf('/.well-known/oauth-') !== 0) {
    return {
      statusCode: 404,
      statusDescription: 'Not Found',
      headers: {
        'content-type': { value: 'application/json' },
        'cache-control': { value: 'no-store' }
      },
      body: '{"error":{"code":"NOT_FOUND","message":"NOT_FOUND"}}'
    };
  }

  // 0. 스캐너 탐색 경로 차단 (오리진까지 보내지 않음)
  for (var i = 0; i < SCANNER_PREFIXES.length; i++) {
    if (uri.indexOf(SCANNER_PREFIXES[i]) === 0) {
      return {
        statusCode: 404,
        statusDescription: 'Not Found',
        headers: {
          'content-type': { value: 'text/html; charset=utf-8' },
          'cache-control': { value: 'no-store' }
        },
        body: "<!DOCTYPE html>\n<html lang=\"ko\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n<title>AUDENIQ STUDIO - 페이지를 찾을 수 없습니다</title>\n<style>\nbody{margin:0;font-family:-apple-system,BlinkMacSystemFont,\"Segoe UI\",Roboto,\"Helvetica Neue\",Arial,sans-serif;background:#f2f2f7;color:#111;display:flex;min-height:100vh;align-items:center;justify-content:center}\nmain{background:#fff;border:0;border-radius:20px;padding:40px 32px;text-align:center;max-width:420px;width:calc(100% - 48px);box-shadow:0 8px 30px rgba(0,0,0,.08)}\n.code{font-size:14px;letter-spacing:.12em;color:#8a8a93;margin:0 0 12px}\nh1{font-size:24px;margin:0 0 12px;line-height:1.4}\np{color:#636366;margin:0 0 24px;line-height:1.6;font-size:15px}\na.btn{display:inline-block;background:#111;color:#fff;text-decoration:none;padding:12px 24px;border-radius:999px;font-size:15px}\na.sub{display:block;margin-top:16px;color:#636366;font-size:14px;text-decoration:none}\n</style>\n</head>\n<body>\n<main>\n<p class=\"code\">404</p>\n<h1>페이지를 찾을 수 없습니다</h1>\n<p>주소가 바뀌었거나 삭제된 페이지입니다. 홈에서 다시 찾아보세요.</p>\n<a class=\"btn\" href=\"/\">홈으로 돌아가기</a>\n<a class=\"sub\" href=\"javascript:history.back()\">이전 페이지로 돌아가기</a>\n</main>\n</body>\n</html>\n"
      };
    }
  }

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
