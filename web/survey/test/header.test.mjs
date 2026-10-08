import { readFileSync as read } from 'node:fs';
import assert from 'node:assert/strict';
import { test } from 'node:test';
const app = read('app/src/App.tsx','utf8');
const css = read('app/src/style.css','utf8');
const shared = read('../shared/design/components.css','utf8');
const source = read('app/index.html','utf8');
const built = read('public/index.html','utf8');
test('survey wordmark navigates only to survey top',()=>{
  assert.match(source, /<body id="top">/);
  assert.match(built, /<body id="top">/);
  assert.match(app, /<a href="#top" className="aq-brand" aria-label="설문 페이지 맨 위로 이동">/);
  assert.match(app, /className="aq-header-link" href="https:\/\/audeniq\.com\/"/);
  assert.doesNotMatch(app, /href="https:\/\/audeniq\.com\/" className="aq-brand"/);
});
test('header comes from the shared AUDENIQ design template',()=>{
  assert.match(css, /@import "\.\.\/\.\.\/\.\.\/shared\/design\/tokens\.css"/);
  assert.match(css, /@import "\.\.\/\.\.\/\.\.\/shared\/design\/components\.css"/);
  assert.match(app, /className=\{`aq-header topbar/);
  assert.match(shared, /\.aq-header__inner\{max-width:var\(--aq-header-max\)/);
  assert.match(shared, /\.aq-brand\{display:block;width:158px/);
  assert.doesNotMatch(css, /\.topbar-inner|\.back-home|--blue:#/);
});
test('built page loads the hashed React bundle and survey endpoint is unchanged',()=>{
  assert.match(built, /<script type="module" crossorigin src="\/assets\/index-[\w-]+\.js"><\/script>/);
  assert.match(built, /<link rel="stylesheet" crossorigin href="\/assets\/index-[\w-]+\.css">/);
  assert.match(app, /fetch\('\/api\/responses'/);
});
