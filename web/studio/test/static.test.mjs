import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,existsSync} from 'node:fs';
const root=new URL('../',import.meta.url);
const read=p=>readFileSync(new URL(p,root),'utf8');
const cfg=JSON.parse(read('wrangler.jsonc'));
test('studio is a separate assets-only Worker',()=>{
 assert.equal(cfg.name,'audeniq-studio');
 assert.equal('main' in cfg,false);
 assert.equal('run_worker_first' in cfg.assets,false);
 assert.equal(cfg.assets.directory,'./public');
});
test('studio remains excluded from search',()=>{
 assert.match(read('public/index.html'),/name="robots" content="noindex,nofollow,noarchive"/);
 assert.match(read('public/_headers'),/X-Robots-Tag: noindex, nofollow, noarchive/);
 assert.match(read('public/robots.txt'),/Disallow: \//);
});
test('studio static assets are present',()=>{
 assert.ok(existsSync(new URL('public/favicon.ico',root)));
 assert.ok(existsSync(new URL('public/assets/AUDENIQ_Logo_Light.svg',root)));
});
