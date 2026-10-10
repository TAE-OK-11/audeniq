// node --test deploy/web/test/web.test.mjs
// 실제 동작(라우팅·헤더)은 nginx로 확인했다: README.md "로컬 확인"
import test from 'node:test';
import assert from 'node:assert/strict';
import { render } from '../nginx.mjs';

test('nginx 설정: 앱마다 server, /api는 백엔드로 비밀·사이트·사용자 IP를 붙여서', async () => {
  const conf = await render();
  for (const host of ['studio.audeniq.com', 'admin.audeniq.com', 'survey.audeniq.com', 'audeniq.com', 'www.audeniq.com']) {
    assert.match(conf, new RegExp(`server_name ${host.replace(/\./g, '\\.')}[ ;]`));
  }
  // envsubst는 서비스 비밀만 채운다
  assert.deepEqual([...new Set(conf.match(/\$\{[A-Z_]+\}/g))], ['${EDGE_SERVICE_SECRET}']);
  for (const site of ['studio', 'admin', 'survey']) assert.match(conf, new RegExp(`X-Audeniq-Site ${site};`));
  assert.equal(conf.match(/X-Audeniq-Client-Ip \$http_cf_connecting_ip;/g).length, 4); // studio /api·/ready, admin, survey
  assert.match(conf, /proxy_pass http:\/\/audeniq_api\/api\/survey\/;/);
  assert.match(conf, /absolute_redirect off;/);
  assert.match(conf, /location \^~ \/assets\/ \{\n\s+add_header X-Robots-Tag/);
  assert.match(conf, /Cache-Control "public, max-age=31536000, immutable"/);
  assert.match(conf, /r2\.cloudflarestorage\.com/);
  assert.match(conf, /location = \/faq \{\n\s+return 308 \/faq\/\$is_args\$args;/);
  assert.match(conf, /server_name www\.audeniq\.com;\n\s+return 308 https:\/\/audeniq\.com\$request_uri;/);
});
