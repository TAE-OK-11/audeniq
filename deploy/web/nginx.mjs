#!/usr/bin/env node
// 개발 단계 화면 서빙: Cloudflare CDN(Workers 없음) → 터널 → 서버의 nginx(web 서비스)
//   node deploy/web/nginx.mjs > audeniq.conf.template
//
// Workers와 CloudFront 함수가 하던 일을 nginx 설정 하나로 만든다 (앱 목록은 deploy/aws/apps.mjs 공용).
// - 화면: SPA 폴백, _redirects, 랜딩 라우팅, `_headers`의 보안 헤더·Cache-Control
// - /api: 백엔드(api:8080)로. 서비스 비밀·사이트 이름·사용자 IP(CF-Connecting-IP)를 nginx가 붙인다.
//   브라우저가 보낸 같은 이름의 헤더는 덮어쓴다. 설문은 /api/x → /api/survey/x.
// ${EDGE_SERVICE_SECRET} 는 nginx 공식 이미지가 컨테이너 시작 때 환경변수로 채운다(envsubst).
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { APPS, cacheGroups, parseHeaders } from '../aws/apps.mjs';
import { headersText } from '../aws/template.mjs';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
export const SITE_ROOT = '/srv/web';
const HSTS = 'max-age=31536000; includeSubDomains';

const quote = v => {
  if (/["\\$]/.test(v)) throw new Error(`nginx 헤더 값에 쓸 수 없는 문자: ${v}`);
  return `"${v}"`;
};

/** _headers 의 "/*" 보안 헤더 + HSTS → add_header 줄들 */
function securityHeaders(name) {
  const all = parseHeaders(headersText(name)).find(r => r.path === '/*')?.headers ?? [];
  return [...all.filter(([h]) => h.toLowerCase() !== 'cache-control'), ['Strict-Transport-Security', HSTS]]
    .map(([h, v]) => `add_header ${h} ${quote(v)} always;`);
}

/** location 하나: nginx는 location 안에 add_header가 있으면 바깥 것을 물려받지 않으므로 매번 전부 쓴다 */
function block(head, headers, cache, body) {
  const lines = [...headers, ...(cache ? [`add_header Cache-Control ${quote(cache)} always;`] : []), ...body];
  return [`location ${head} {`, ...lines.map(l => `  ${l}`), '}'];
}

const redirect = to => [`return 308 ${to}$is_args$args;`];

function apiBlock(path, site, prefix) {
  const target = prefix ? `http://audeniq_api${prefix}/` : 'http://audeniq_api';
  return [
    `location ${path} {`,
    `  proxy_pass ${target};`,
    '  proxy_http_version 1.1;',
    '  proxy_set_header Connection "";',
    '  proxy_set_header Host $host;',
    '  proxy_set_header X-Audeniq-Service "${EDGE_SERVICE_SECRET}";',
    `  proxy_set_header X-Audeniq-Site ${site};`,
    // 터널로만 들어오므로 Cloudflare가 매 요청 덮어쓰는 CF-Connecting-IP를 믿는다
    '  proxy_set_header X-Audeniq-Client-Ip $http_cf_connecting_ip;',
    '  proxy_set_header X-Forwarded-For "";',
    '  proxy_set_header X-Real-Ip "";',
    // 실시간 알림(SSE)은 버퍼링 없이, keep-alive(20초)보다 넉넉히
    '  proxy_buffering off;',
    '  proxy_read_timeout 120s;',
    '  client_max_body_size 1m;',
    `  add_header Strict-Transport-Security ${quote(HSTS)} always;`,
    '}',
  ];
}

function appServer(name) {
  const app = APPS[name];
  const h = securityHeaders(name);
  const groups = cacheGroups(parseHeaders(headersText(name)));
  const out = ['server {', '  listen 8080;', `  server_name ${app.aliases.join(' ')};`, `  root ${SITE_ROOT}/${name};`];
  const lines = [];
  if (app.api.includes('/api/*')) lines.push(...apiBlock('^~ /api/', app.site, app.apiPrefix));
  for (const p of app.api.filter(p => p !== '/api/*')) lines.push(...apiBlock(`= ${p}`, app.site));

  if (name === 'studio') {
    // web/studio/public/_redirects
    for (const from of ['/index.html', '/studio', '/studio/', '/studio/index.html']) lines.push(...block(`= ${from}`, [], null, redirect('/')));
  }
  for (const { include, cacheControl } of groups) {
    lines.push(...block(`^~ /${include.replace(/\*$/, '')}`, h, cacheControl, ['try_files $uri =404;']));
  }
  if (name === 'survey') {
    // Workers의 auto-trailing-slash + 404-page
    lines.push(...block('= /index.html', [], null, redirect('/')));
    lines.push(...block('= /', h, 'no-cache', ['try_files /index.html =404;']));
    lines.push(...block('/', h, 'no-cache', ['try_files $uri $uri/index.html =404;']));
    out.push('  error_page 404 /404.html;');
  } else {
    // 파일은 그대로(없으면 404), 확장자 없는 화면 경로는 index.html (SPA)
    lines.push(...block('~ \\.[A-Za-z0-9]+$', h, 'no-cache', ['try_files $uri =404;']));
    lines.push(...block('/', h, 'no-cache', ['try_files /index.html =404;']));
  }
  out.push(...lines.map(l => `  ${l}`), '}');
  return out;
}

async function landingServers() {
  const app = APPS.landing;
  const { PAGE_PATHS, FILE_PATHS } = await import(pathToFileURL(resolve(ROOT, app.routes)).href);
  const h = [...securityHeaders('landing'), 'add_header Cache-Control "no-cache" always;', 'add_header X-Robots-Tag $landing_robots always;'];
  const lines = [
    // ?site=studio 와 /studio 경로는 React STUDIO로 (web/landing/src/worker.js studioRedirect)
    'if ($arg_site = "studio") { return 308 https://studio.audeniq.com/; }',
    ...block('= /health', [], 'no-store', ['default_type application/json;', 'return 200 \'{"ok":true,"service":"AUDENIQ Web","host":"$host"}\';']),
    ...block('= /studio', [], null, ['return 308 https://studio.audeniq.com/$is_args$args;']),
    ...block('= /studio/index.html', [], null, ['return 308 https://studio.audeniq.com/$is_args$args;']),
    ...block('~ ^/studio(/.*)$', [], null, ['return 308 https://studio.audeniq.com$1$is_args$args;']),
    ...block('= /index.html', [], null, redirect('/')),
  ];
  for (const page of PAGE_PATHS) {
    if (page !== '/') {
      lines.push(...block(`= ${page.slice(0, -1)}`, [], null, redirect(page)));
      lines.push(...block(`= ${page}index.html`, [], null, redirect(page)));
    }
    lines.push(...block(`= ${page}`, h, null, [`try_files ${page}index.html =404;`]));
  }
  for (const file of FILE_PATHS) lines.push(...block(`= ${file}`, h, null, ['try_files $uri =404;']));
  lines.push(...block('^~ /assets/', h, null, ['try_files $uri =404;']));
  // 게시된 문서만 — 모르는 경로는 진짜 404
  lines.push(...block('/', h, null, ['default_type text/plain;', 'return 404 "Not found\\n";']));
  return [
    'server {', '  listen 8080;', '  server_name www.audeniq.com;', '  return 308 https://audeniq.com$request_uri;', '}',
    'server {', '  listen 8080;', '  server_name audeniq.com;', `  root ${SITE_ROOT}/landing;`,
    ...lines.map(l => `  ${l}`), '}',
  ];
}

export async function render() {
  return [
    '# 생성됨: node deploy/web/nginx.mjs — 직접 고치지 말고 생성기를 고치세요',
    'map $status $landing_robots { ~^[45] "noindex, nofollow, noarchive"; default ""; }',
    'upstream audeniq_api { server api:8080; keepalive 16; }',
    'server_tokens off;',
    // 터널 안쪽은 http:8080 — 리다이렉트 Location에 그 주소가 새지 않게 상대 경로로
    'absolute_redirect off;',
    'charset utf-8;',
    'charset_types text/css application/javascript application/json image/svg+xml text/plain text/xml;',
    'gzip on;',
    'gzip_types text/css application/javascript application/json image/svg+xml text/plain text/xml application/xml;',
    'gzip_min_length 1024;',
    // 모르는 호스트는 거절
    'server { listen 8080 default_server; return 404; }',
    ...['studio', 'admin', 'survey'].flatMap(appServer),
    ...(await landingServers()),
    '',
  ].join('\n');
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.stdout.write(await render());
}
