import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,existsSync} from 'node:fs';
const root=new URL('../',import.meta.url);
const read=p=>readFileSync(new URL(p,root),'utf8');
const cfg=JSON.parse(read('wrangler.jsonc'));
test('studio Worker serves the React build and D1 content',()=>{
 assert.equal(cfg.name,'audeniq-studio');
 assert.equal(cfg.main,'./worker.js');
 assert.equal(cfg.assets.directory,'./public');
 // /login 같은 화면 경로는 index.html, /api/*는 항상 Worker가 처리
 assert.equal(cfg.assets.not_found_handling,'single-page-application');
 assert.deepEqual(cfg.assets.run_worker_first,['/api/*']);
 const db=cfg.d1_databases.find(d=>d.binding==='CONTENT_DB');
 assert.ok(db,'CONTENT_DB binding');
 assert.equal(db.migrations_dir,'migrations');
 assert.ok(existsSync(new URL('migrations/0001_content.sql',root)));
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
