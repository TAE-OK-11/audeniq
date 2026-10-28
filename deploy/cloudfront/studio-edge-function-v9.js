// Prepared v9: jsDelivr AUDENIQ Sans ss20 logo brand, plain-text fallback; v7 behaviour preserved. NOT DEPLOYED.

var SCANNER_PREFIXES = [
  '/.env',
  '/.git',
  '/.svn',
  '/.aws/',
  '/.htaccess',
  '/.htpasswd',
  '/backup',
  '/wp-',
  '/wordpress',
  '/xmlrpc',
  '/phpmyadmin',
  '/pma',
  '/myadmin',
  '/adminer',
  '/phpinfo',
  '/vendor',
  '/cgi-bin',
  '/thinkphp',
  '/fckeditor',
  '/ckeditor',
  '/elfinder',
  '/kcfinder',
  '/filemanager',
  '/install',
  '/setup',
  '/sqladmin',
  '/dbadmin',
  '/webadmin',
  '/shell',
  '/c99',
  '/r57',
  '/composer.json',
  '/composer.lock'
];
var SCANNER_SUFFIXES = ['.php', '.php5', '.phar', '.phtml', '.asp', '.aspx', '.jsp', '.cgi', '.bak'];
var SCANNER_SUBSTRINGS = ['wp-config', 'wp-includes', 'wp-content', 'wlwmanifest', 'xmlrpc'];
var BOT_ALLOW = ['googlebot', 'bingbot', 'duckduckbot', 'baiduspider', 'yandexbot', 'yeti', 'daumoa', 'applebot', 'gptbot', 'chatgpt-user', 'claudebot', 'anthropic-ai', 'perplexitybot', 'ccbot', 'cohere-ai', 'bytespider', 'meta-externalagent', 'facebookexternalhit', 'twitterbot', 'linkedinbot', 'slackbot', 'discordbot', 'telegrambot'];
var BOT_BLOCK = ['bot', 'crawl', 'spider', 'scrape', 'scanner', 'semrush', 'ahrefs', 'mj12', 'dotbot', 'blex', 'dataforseo', 'sistrix', 'headless', 'phantomjs', 'selenium', 'puppeteer', 'nmap', 'nikto', 'sqlmap', 'masscan', 'zgrab', 'shodan', 'censys'];

function errPage(code, title, heading, text, links) {
  return '<!DOCTYPE html><html lang="ko"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>' + title + '</title><link rel="stylesheet" href="https://cdn.jsdelivr.net/gh/TAE-OK-11/audeniq-font@308d641281443c6172d94b50785d43890f5aafde/css/audeniq-sans-web.css"><style>' + "*{box-sizing:border-box}body{margin:0;font-family:\"Audeniq Sans\",\"Audeniq Sans Full\",-apple-system,BlinkMacSystemFont,\"Apple SD Gothic Neo\",\"Noto Sans KR\",\"Malgun Gothic\",sans-serif;background:#fff;color:#171B29;min-height:100vh;min-height:100dvh;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:22px;padding:32px 16px;word-break:keep-all;overflow-wrap:anywhere}.brand{display:inline-flex;align-items:center;gap:10px;color:#171B29;text-decoration:none;font-size:22px;font-weight:800;letter-spacing:-.02em}.brand span{font-size:11px;letter-spacing:.14em;color:#3B63F3;background:#F5F8FF;border-radius:8px;padding:6px 10px}main{width:100%;max-width:520px;margin-bottom:6vh;padding:40px 22px 30px;border-radius:28px;background:#F5F8FF;text-align:center}.icon{display:grid;place-items:center;width:60px;height:60px;border-radius:50%;margin:0 auto 18px;background:#fff;color:#3B63F3;box-shadow:0 0 0 8px #3b63f30f}.code{display:table;margin:0 auto 10px;padding:2px 9px;border-radius:32px;background:#fff;color:#3B63F3;font-size:12px;font-weight:800;letter-spacing:.02em}h1{margin:0;font-size:20px;font-weight:700;line-height:1.45;letter-spacing:-.02em}p{margin:8px auto 0;max-width:420px;color:#646E84;font-size:14px;line-height:1.7}.actions{margin-top:26px}a.btn{display:inline-block;min-width:120px;padding:12px 20px;border-radius:14px;background:#3B63F3;color:#fff;text-decoration:none;font-size:14px;font-weight:700}a.btn:hover{background:#2C56E0}a.sub{display:block;margin-top:16px;color:#4D5970;font-size:14px;text-decoration:none}a:focus-visible{outline:3px solid #6F91FF;outline-offset:4px}@media(min-width:768px){main{padding:56px 32px 40px}h1{font-size:23px}p{font-size:15px}}.brand span.aq-logo{background:transparent;color:#171B29;font-size:22px;font-weight:800;letter-spacing:0;padding:0;border-radius:0;font-family:\"Audeniq Sans\",\"Audeniq Sans Full\",-apple-system,BlinkMacSystemFont,\"Apple SD Gothic Neo\",\"Noto Sans KR\",\"Malgun Gothic\",sans-serif;font-feature-settings:\"ss20\";}" + '</style></head><body>' + "<a class=\"brand\" href=\"/\" aria-label=\"AUDENIQ STUDIO \ud648\"><span class=\"aq-logo\">AUDENIQ</span><span>STUDIO</span></a><main><span class=\"icon\" aria-hidden=\"true\"><svg viewBox=\"0 0 24 24\" width=\"28\" height=\"28\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.8\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><circle cx=\"11\" cy=\"11\" r=\"7\"/><path d=\"m20 20-3.5-3.5M11 8v3M11 14h.01\"/></svg></span>" + '<p class="code">' + code + '</p><h1>' + heading + '</h1><p>' + text + '</p><div class="actions">' + links + '</div></main></body></html>';
}

function handler(event) {
  var request = event.request;
  var uri = request.uri || '/';

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

  var lower = uri.toLowerCase();
  var isScanner = false;
  for (var j = 0; j < SCANNER_SUFFIXES.length && !isScanner; j++) {
    if (lower.length > SCANNER_SUFFIXES[j].length &&
        lower.lastIndexOf(SCANNER_SUFFIXES[j]) === lower.length - SCANNER_SUFFIXES[j].length) {
      isScanner = true;
    }
  }
  for (var i = 0; i < SCANNER_PREFIXES.length && !isScanner; i++) {
    if (lower.indexOf(SCANNER_PREFIXES[i]) === 0) {
      isScanner = true;
    }
  }
  for (var k = 0; k < SCANNER_SUBSTRINGS.length && !isScanner; k++) {
    if (lower.indexOf(SCANNER_SUBSTRINGS[k]) >= 0) {
      isScanner = true;
    }
  }
  if (isScanner) {
      return {
        statusCode: 404,
        statusDescription: 'Not Found',
        headers: {
          'content-type': { value: 'text/html; charset=utf-8' },
          'cache-control': { value: 'no-store' }
        },
        body: errPage('404', 'AUDENIQ STUDIO - 페이지를 찾을 수 없습니다', '페이지를 찾을 수 없습니다', '주소가 바뀌었거나 삭제된 페이지입니다. 홈에서 다시 찾아보세요.', '<a class="btn" href="/">홈으로 돌아가기</a>\n<a class="sub" href="javascript:history.back()">이전 페이지로 돌아가기</a>')
      };
  }

  var uaRaw = request.headers['user-agent'] ? request.headers['user-agent'].value : '';
  var ua = uaRaw.toLowerCase();
  var isGoodBot = false;
  for (var b = 0; b < BOT_ALLOW.length && !isGoodBot; b++) {
    if (ua.indexOf(BOT_ALLOW[b]) >= 0) { isGoodBot = true; }
  }
  var isBadBot = false;
  if (!isGoodBot) {
    for (var c = 0; c < BOT_BLOCK.length && !isBadBot; c++) {
      if (ua.indexOf(BOT_BLOCK[c]) >= 0) { isBadBot = true; }
    }
  }
  if (isBadBot) {
    return {
      statusCode: 403,
      statusDescription: 'Forbidden',
      headers: {
        'content-type': { value: 'text/html; charset=utf-8' },
        'cache-control': { value: 'no-store' }
      },
      body: errPage('403', 'AUDENIQ STUDIO - 접근이 제한되었습니다', '접근이 제한되었습니다', '자동화된 수집 요청은 허용된 봇만 이용할 수 있습니다.', '<a class="btn" href="/">홈으로 돌아가기</a>')
    };
  }
  var headers = request.headers;

  Object.keys(headers).forEach(function (name) {
    if (name.toLowerCase().indexOf('x-audeniq-') === 0) {
      delete headers[name];
    }
  });

  headers['x-audeniq-client-ip'] = { value: event.viewer.ip };

  return request;
}
