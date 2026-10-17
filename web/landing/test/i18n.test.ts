// Crawler-view checks for every language version: what a search engine receives
// from the Worker without running JavaScript.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import worker from '../src/worker.ts';
import { PAGE_PATHS } from '../src/public-routes.ts';
import { strayHangul, translateHtml } from '../scripts/i18n.ts';
import type { TransportEnv } from '../../shared/worker-types.ts';
import type { TranslationMap } from '../scripts/i18n.ts';
import type { Locale, LocaleFile, LocaleRegistry } from '../scripts/content-types.ts';

/** The JSON-LD fields these checks read */
interface GraphNode { '@type'?: string | string[]; '@id'?: string; name?: string; alternateName?: string; url?: string; inLanguage?: string; mainEntity?: { name: string; acceptedAnswer: { text: string } }[] }

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const read = (path: string): string => readFileSync(join(root, path), 'utf8');
const origin = 'https://audeniq.com';
const env: TransportEnv = {ASSETS:{fetch:async (request: Request)=>{
  const file = join(root, 'public', new URL(request.url).pathname);
  return existsSync(file) ? new Response(readFileSync(file), {headers:{'Content-Type':'text/html; charset=utf-8'}}) : new Response('Not found', {status:404});
}}};
const get = (path: string): Promise<Response> => worker.fetch(new Request(origin + path), env);
const registry: LocaleRegistry = JSON.parse(read('content/locales.json'));
const published = registry.locales.filter(locale => locale.status === 'published');
const decode = (text: string): string => text.replace(/&amp;/g,'&').replace(/&quot;/g,'"').replace(/&#39;/g,"'").replace(/&lt;/g,'<').replace(/&gt;/g,'>');
const attr = (tag: string, name: string): string | undefined => tag.match(new RegExp(`\\s${name}="([^"]*)"`))?.[1];
const keyOf = (path: string): { locale: Locale; key: string } => {
  // build-seo.ts refuses to build unless the default language is published at "/".
  const locale = published.filter(item => item.prefix !== '/').find(item => path.startsWith(item.prefix)) || published.find(item => item.prefix === '/')!;
  return { locale, key: '/' + path.slice(locale.prefix.length) };
};
const pages = new Map<string, { status: number; robots: string | null; html: string }>();
for (const path of PAGE_PATHS) {
  const response = await get(path);
  pages.set(path, { status: response.status, robots: response.headers.get('X-Robots-Tag'), html: await response.text() });
}
const alternates = (html: string): Record<string, string> => Object.fromEntries([...html.matchAll(/<link rel="alternate" hreflang="([^"]+)" href="([^"]+)">/g)].map(match => [match[1], match[2]]));

test('every published language has a crawlable homepage and only published languages are routed', () => {
  for (const locale of published) assert.ok(PAGE_PATHS.includes(locale.prefix), locale.code);
  for (const locale of registry.locales.filter(item => item.status !== 'published')) {
    assert.ok(!PAGE_PATHS.some(path => path.startsWith(locale.prefix)), `draft ${locale.code} must not be public`);
  }
  for (const path of PAGE_PATHS) assert.ok(!PAGE_PATHS.includes(path.replace(/\/$/, '')), 'one URL per page');
});

test('html lang, direction, canonical, title and description are correct per language', () => {
  const titles = new Set(), descriptions = new Set();
  for (const [path, page] of pages) {
    const { locale } = keyOf(path);
    assert.equal(page.status, 200, path);
    assert.equal(page.robots, null, path);
    const html = page.html;
    const htmlTag = html.match(/<html[^>]*>/)?.[0];
    assert.ok(htmlTag, path);
    assert.equal(attr(htmlTag, 'lang'), locale.lang, path);
    assert.equal(attr(htmlTag, 'dir') || 'ltr', locale.dir, path);
    assert.equal((html.match(/<link rel="canonical"/g) || []).length, 1, path);
    assert.ok(html.includes(`<link rel="canonical" href="${origin}${path}">`), `self canonical ${path}`);
    assert.match(html, /<meta name="robots" content="index, follow/, path);
    const rawTitle = html.match(/<title>([^<]+)<\/title>/)?.[1], rawDescription = html.match(/<meta name="description" content="([^"]+)">/)?.[1];
    assert.ok(rawTitle && rawDescription, path);
    const title = decode(rawTitle);
    const description = decode(rawDescription);
    assert.ok(title.length >= 15 && title.length <= 90, `title length ${path}: ${title}`);
    assert.ok(description.length >= 50, `description ${path}`);
    assert.ok(!titles.has(title), `duplicate title ${title}`); titles.add(title);
    assert.ok(!descriptions.has(description), `duplicate description ${path}`); descriptions.add(description);
    assert.ok(html.includes(`<meta property="og:locale" content="${locale.ogLocale}">`), `og:locale ${path}`);
    assert.ok(html.includes(`<meta property="og:url" content="${origin}${path}">`) || !html.includes('og:url'), `og:url ${path}`);
    assert.equal((html.match(/<h1(?:\s|>)/g) || []).length, 1, `one H1 ${path}`);
  }
});

test('hreflang annotations are complete, reciprocal and match the sitemap', () => {
  const sitemap = read('public/sitemap.xml');
  for (const [path, page] of pages) {
    const links = alternates(page.html);
    const { locale } = keyOf(path);
    assert.equal(links[locale.hreflang], origin + path, `self hreflang ${path}`);
    assert.ok(links['x-default'], `x-default ${path}`);
    assert.ok(PAGE_PATHS.includes(links['x-default'].slice(origin.length)), `x-default target ${path}`);
    for (const [hreflang, href] of Object.entries(links)) {
      const target = href.slice(origin.length);
      assert.ok(PAGE_PATHS.includes(target), `hreflang target is published: ${href}`);
      if (hreflang === 'x-default') continue;
      // `pages` holds every PAGE_PATHS entry and target was just checked to be one.
      assert.deepEqual(alternates(pages.get(target)!.html), links, `reciprocal hreflang ${path} ↔ ${target}`);
      assert.equal(keyOf(target).locale.hreflang, hreflang, `hreflang code ${href}`);
    }
    const entry = sitemap.match(new RegExp(`<url><loc>${origin}${path.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}</loc>[\\s\\S]*?</url>`))?.[0];
    assert.ok(entry, `sitemap entry ${path}`);
    const mapped = Object.fromEntries([...entry.matchAll(/hreflang="([^"]+)" href="([^"]+)"/g)].map(match => [match[1], match[2]]));
    assert.deepEqual(mapped, links, `sitemap alternates ${path}`);
  }
  assert.equal((sitemap.match(/<loc>/g) || []).length, PAGE_PATHS.length);
});

test('translations are complete: no Korean left outside the brand and language names', () => {
  for (const [path, page] of pages) {
    if (keyOf(path).locale.code === registry.default) continue;
    assert.deepEqual(strayHangul(page.html), [], path);
  }
  const source = read('index.html').split('</head>')[1].replace(/<!-- LANGUAGE_LINKS_START -->[\s\S]*?<!-- LANGUAGE_LINKS_END -->/, '');
  for (const locale of published.filter(item => item.code !== registry.default)) {
    // Published languages always carry homepage translations (build-seo.ts fails otherwise).
    const file: LocaleFile & { home: TranslationMap } = JSON.parse(read(`content/i18n/${locale.code}.json`));
    const { missing, unused } = translateHtml(source, file.home);
    assert.deepEqual(missing, [], `${locale.code} missing`);
    assert.deepEqual(unused, [], `${locale.code} has stale translations`);
    for (const value of Object.values(file.home)) assert.doesNotMatch(value, /<(?!br\b)/i, `${locale.code} markup`);
  }
});

test('structured data matches each language page and stays factual', () => {
  const koFaq = (pages.get('/')!.html.match(/<details><summary>/g) || []).length;
  for (const [path, page] of pages) {
    const { locale, key } = keyOf(path);
    const blocks = [...page.html.matchAll(/<script type="application\/ld\+json">([\s\S]*?)<\/script>/g)];
    assert.equal(blocks.length, 1, `one JSON-LD block ${path}`);
    const graph: GraphNode[] = JSON.parse(blocks[0][1])['@graph'];
    const org = graph.find(node => node['@type'] === 'Organization');
    assert.ok(org, `Organization ${path}`);
    assert.equal(org['@id'], `${origin}/#organization`);
    assert.equal(org.name, 'AUDENIQ');
    assert.equal(org.alternateName, '어데닉');
    const webpage = graph.find(node => node['@id'] === `${origin}${path}#webpage`);
    assert.ok(webpage, `WebPage ${path}`);
    assert.equal(webpage.inLanguage, locale.lang, path);
    assert.equal(webpage.url, origin + path);
    const body = decode(page.html.split('</head>')[1]);
    for (const question of webpage.mainEntity || []) {
      assert.ok(body.includes(question.name), `visible question ${path}: ${question.name}`);
      assert.ok(body.includes(question.acceptedAnswer.text), `visible answer ${path}`);
    }
    if (key === '/') assert.equal(webpage.mainEntity?.length, koFaq, `FAQ parity ${path}`);
    const json = JSON.stringify(graph);
    assert.doesNotMatch(json, /"(?:price|offers|aggregateRating|review|award)"\s*:/i, path);
    const visible = body.replace(/<script\b[^>]*>[\s\S]*?<\/script>/g, '').replace(/<style\b[^>]*>[\s\S]*?<\/style>/g, '').replace(/<[^>]*>/g, '');
    assert.doesNotMatch(visible, /LIMBO|₩|\d+\s*%/i, path);
  }
});

test('language menu links to the same page in every available language', async () => {
  for (const [path, page] of pages) {
    const nav = page.html.match(/<nav class="footer-languages"[\s\S]*?<\/nav>/)?.[0];
    assert.ok(nav, `language menu ${path}`);
    const links = [...nav.matchAll(/<a href="([^"]+)" hreflang="([^"]+)" lang="([^"]+)"/g)];
    const expected = Object.entries(alternates(page.html)).filter(([code]) => code !== 'x-default');
    assert.equal(links.length, expected.length, path);
    for (const [, href, hreflang] of links) {
      assert.equal(alternates(page.html)[hreflang], origin + href, `${path} → ${href}`);
      assert.equal((await get(href)).status, 200);
    }
    assert.equal((nav.match(/aria-current="page"/g) || []).length, 1);
  }
});

test('language URLs have one canonical form and no forced language redirect', async () => {
  for (const locale of published.filter(item => item.prefix !== '/')) {
    const bare = await get(locale.prefix.slice(0, -1) + '?utm=x');
    assert.equal(bare.status, 308);
    assert.equal(bare.headers.get('location'), `${origin}${locale.prefix}?utm=x`);
    assert.equal((await get(`${locale.prefix}index.html`)).status, 308);
    assert.equal((await get(`${locale.prefix}missing/`)).status, 404);
  }
  // Accept-Language never changes the response: every URL serves its own language.
  const ja = await worker.fetch(new Request(`${origin}/`, {headers:{'Accept-Language':'ja,en;q=0.8'}}), env);
  assert.equal(ja.status, 200);
  assert.match(await ja.text(), /<html lang="ko">/);
});

test('robots.txt and llms.txt expose every language without blocking public pages', () => {
  const robots = read('public/robots.txt');
  const disallowed = [...robots.matchAll(/^Disallow: (.+)$/gm)].map(match => match[1].trim());
  for (const path of PAGE_PATHS) assert.ok(!disallowed.some(rule => path.startsWith(rule)), `robots blocks ${path}`);
  const llms = read('public/llms.txt');
  for (const locale of published) assert.ok(llms.includes(`${origin}${locale.prefix}`), `llms.txt ${locale.code}`);
  assert.match(llms, /어데닉/);
  assert.match(llms, /not yet accepting releases/);
});

test('survey calls to action go to the AUDENIQ survey site', () => {
  for (const [path, page] of pages) {
    assert.doesNotMatch(page.html, /docs\.google\.com\/forms/, path);
    if (keyOf(path).key === '/') assert.equal((page.html.match(/href="https:\/\/survey\.audeniq\.com\/"/g) || []).length, 2, path);
  }
});
