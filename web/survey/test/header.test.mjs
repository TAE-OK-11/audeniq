import { readFileSync as read } from 'node:fs';
import assert from 'node:assert/strict';
import { test } from 'node:test';
const html = read('public/index.html','utf8');
const css = read('public/style.css','utf8');
test('survey wordmark navigates only to survey top',()=>{
  assert.match(html, /<body id="top">/);
  assert.match(html, /<a href="#top" class="brand" aria-label="설문 페이지 맨 위로 이동">/);
  assert.match(html, /class="back-home" href="https:\/\/audeniq\.com\/"/);
  assert.doesNotMatch(html, /href="https:\/\/audeniq\.com\/" class="brand"/);
});
test('wordmark position follows homepage breakpoints',()=>{
  assert.match(css, /max-width:1240px;min-height:80px/);
  assert.match(css, /\.brand\{display:block;width:158px/);
  assert.match(css, /@media \(min-width:761px\) and \(max-width:900px\)/);
  assert.match(css, /\.topbar \.brand\{width:138px;margin-left:-8px\}/);
  assert.match(css, /\.topbar \.header-label\{display:none\}/);
});
test('static css cache-busted and survey endpoints unchanged',()=>{
  assert.match(html, /style\.css\?v=5\.1/);
  assert.match(read('public/app.js','utf8'), /fetch\('\/api\/responses'/);
});
