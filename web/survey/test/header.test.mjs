import { readFileSync as read } from 'node:fs';
import assert from 'node:assert/strict';
import { test } from 'node:test';
const app = read('app/src/App.tsx','utf8');
const css = read('app/src/style.css','utf8');
const source = read('app/index.html','utf8');
const built = read('public/index.html','utf8');
test('survey wordmark navigates only to survey top',()=>{
  assert.match(source, /<body id="top">/);
  assert.match(built, /<body id="top">/);
  assert.match(app, /<a href="#top" className="brand" aria-label="설문 페이지 맨 위로 이동">/);
  assert.match(app, /className="back-home" href="https:\/\/audeniq\.com\/"/);
  assert.doesNotMatch(app, /href="https:\/\/audeniq\.com\/" className="brand"/);
});
test('wordmark position follows homepage breakpoints',()=>{
  assert.match(css, /max-width:1240px;min-height:80px/);
  assert.match(css, /\.brand\{display:block;width:158px/);
  assert.match(css, /@media \(min-width:761px\) and \(max-width:900px\)/);
  assert.match(css, /\.topbar \.brand\{width:138px;margin-left:-8px\}/);
  assert.match(css, /\.topbar \.header-label\{display:none\}/);
});
test('built page loads the hashed React bundle and survey endpoint is unchanged',()=>{
  assert.match(built, /<script type="module" crossorigin src="\/assets\/index-[\w-]+\.js"><\/script>/);
  assert.match(built, /<link rel="stylesheet" crossorigin href="\/assets\/index-[\w-]+\.css">/);
  assert.match(app, /fetch\('\/api\/responses'/);
});
