import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,existsSync} from 'node:fs';
const root=new URL('../',import.meta.url);
const read=(p:string):string=>readFileSync(new URL(p,root),'utf8');
const cfg=JSON.parse(read('wrangler.jsonc'));
test('studio Worker is a proxy only; the backend serves the React build',()=>{
 // 예전 audeniq-studio(정적 서빙)는 DNS 전환 뒤 롤백용으로 남긴다: 같은 이름으로 덮어쓰지 않는다
 assert.equal(cfg.name,'audeniq-studio-proxy');
 assert.equal(cfg.main,'./worker.ts');
 // 정적 에셋 바인딩 없음: 화면 파일·SPA 폴백은 백엔드 (crates/core/src/studio_static.rs)
 assert.equal(cfg.assets,undefined);
 // 공지·이벤트·점검은 백엔드 Postgres (migrations/0086_site_content.sql)
 assert.equal(cfg.d1_databases,undefined);
});
test('studio remains excluded from search',()=>{
 assert.match(read('public/index.html'),/name="robots" content="noindex,nofollow,noarchive"/);
 assert.match(read('public/_headers'),/X-Robots-Tag: noindex, nofollow, noarchive/);
 assert.match(read('public/robots.txt'),/Disallow: \//);
});
test('studio static assets are present',()=>{
 assert.ok(existsSync(new URL('public/favicon.ico',root)));
 assert.ok(existsSync(new URL('public/static/AUDENIQ_Logo_Light.svg',root)));
});
