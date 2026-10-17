import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import worker from '../src/worker.ts';
import { PAGE_PATHS } from '../src/public-routes.ts';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const read = path=>readFileSync(join(root,path),'utf8');
const types = {html:'text/html',css:'text/css',xml:'application/xml',txt:'text/plain',json:'application/json'};
const env = {ASSETS:{fetch:async request=>{
  const path = new URL(request.url).pathname;
  const file = join(root,'public',path);
  return existsSync(file) ? new Response(readFileSync(file),{headers:{'Content-Type':types[path.split('.').pop()]||'application/octet-stream'}}) : new Response('Not found',{status:404});
}}};
const get = (path,host='audeniq.com')=>worker.fetch(new Request(`https://${host}${path}`),env);
const decode = text=>text.replace(/&amp;/g,'&').replace(/&quot;/g,'"').replace(/&#39;/g,"'").replace(/&lt;/g,'<').replace(/&gt;/g,'>');

test('every published page is crawlable with matching visible FAQs and schema',async()=>{
  assert.ok(PAGE_PATHS.length>=6);
  const titles = new Set();
  for (const path of PAGE_PATHS) {
    const response = await get(path);
    assert.equal(response.status,200,path);
    assert.match(response.headers.get('Content-Type'),/text\/html/);
    assert.equal(response.headers.get('X-Robots-Tag'),null);
    const html = await response.text();
    assert.equal((html.match(/<h1(?:\s|>)/g)||[]).length,1,path);
    assert.ok(html.includes(`rel="canonical" href="https://audeniq.com${path}"`),path);
    const title = html.match(/<title>([^<]+)<\/title>/)[1];
    assert.ok(!titles.has(title)); titles.add(title);
    const graph = JSON.parse(html.match(/<script type="application\/ld\+json">([\s\S]*?)<\/script>/)[1])['@graph'];
    assert.equal(graph[0]['@id'],'https://audeniq.com/#organization');
    const body = decode(html.split('</head>')[1]);
    for (const question of graph.find(node=>node.mainEntity)?.mainEntity||[]) {
      assert.ok(body.includes(question.name),`Visible question ${path}`);
      assert.ok(body.includes(question.acceptedAnswer.text),`Visible answer ${path}`);
    }
    const visibleText = body.replace(/<script\b[^>]*>[\s\S]*?<\/script>/g,'').replace(/<style\b[^>]*>[\s\S]*?<\/style>/g,'').replace(/<[^>]*>/g,'');
    assert.doesNotMatch(visibleText,/LIMBO|수수료|₩|\d+\s*%/i,path);
    assert.doesNotMatch(JSON.stringify(graph),/"(?:price|offers|aggregateRating|review)"\s*:/i,path);
    const preview = await get(path,'audeniq-web.example.workers.dev');
    assert.equal(preview.status,200);
    assert.match(preview.headers.get('X-Robots-Tag'),/noindex/);
    for (const match of html.matchAll(/(?:href|src)="(\/(?!\/)[^"]*)"/g)) {
      const target = match[1].split(/[?#]/)[0];
      const linked = await get(target);
      assert.ok([200,308].includes(linked.status),`Broken link ${path}: ${target}`);
    }
  }
});

test('new page aliases redirect once and preserve query strings',async()=>{
  for (const path of PAGE_PATHS.filter(path=>path!='/')) {
    for (const alias of [path.slice(0,-1),`${path}index.html`]) {
      for (const host of ['audeniq.com','www.audeniq.com']) {
        const response = await get(`${alias}?ref=test`,host);
        assert.equal(response.status,308);
        assert.equal(response.headers.get('location'),`https://audeniq.com${path}?ref=test`);
      }
    }
  }
  for (const path of ['/blog/','/publishing/missing/','/unknown.txt','/admin','/api/private']) {
    assert.equal((await get(path)).status,404,path);
  }
});

test('discovery resources contain exactly the published public pages',async()=>{
  const sitemap = read('public/sitemap.xml');
  const urls = [...sitemap.matchAll(/<loc>([^<]+)<\/loc>/g)].map(match=>match[1]);
  assert.deepEqual(urls,PAGE_PATHS.map(path=>`https://audeniq.com${path}`));
  assert.equal((sitemap.match(/<lastmod>\d{4}-\d{2}-\d{2}<\/lastmod>/g)||[]).length,PAGE_PATHS.length);
  for (const name of ['llms.txt','llms-full.txt','feed.xml','robots.txt','sitemap.xml']) {
    assert.equal(read(name),read(`public/${name}`),name);
    assert.equal((await get(`/${name}`)).status,200);
  }
  const full = read('public/llms-full.txt');
  const data = JSON.parse(read('content/pages.json'));
  for (const page of data.pages) assert.ok(full.includes(page.answer));
  const feed = read('public/feed.xml');
  assert.equal((feed.match(/<item>/g)||[]).length,5);
  assert.doesNotMatch(full,/LIMBO|수수료|₩|\d+\s*%/i);
  const key = read('content/indexnow-key.txt').trim();
  assert.match(key,/^[a-f0-9]{32}$/);
  assert.equal((await (await get(`/${key}.txt`)).text()).trim(),key);
});

test('robots allows every crawler, including AI training bots',()=>{
  const robots = read('robots.txt');
  const groups = robots.trim().split(/\n\s*\n/);
  // One `*` group: GPTBot, ClaudeBot, Google-Extended, CCBot … fall under it and are allowed.
  assert.deepEqual(robots.match(/^User-agent: .+$/gm),['User-agent: *']);
  const all = groups.find(group=>group.split('\n').includes('User-agent: *'));
  assert.match(all,/Allow: \/\n/);
  assert.doesNotMatch(robots,/Disallow: \/\s*$/m);
  for (const path of ['/studio','/admin','/api','/login','/health']) assert.ok(all.includes(`Disallow: ${path}\n`)||all.endsWith(`Disallow: ${path}`));
  assert.match(robots,/Sitemap: https:\/\/audeniq\.com\/sitemap\.xml/);
});
