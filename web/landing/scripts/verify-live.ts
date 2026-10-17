import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import type { Locale, LocaleRegistry, SiteManifest } from './content-types.ts';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const manifest: SiteManifest = JSON.parse(readFileSync(join(root,'content/generated-manifest.json'),'utf8'));
const origin = 'https://audeniq.com';
const locales = (JSON.parse(readFileSync(join(root,'content/locales.json'),'utf8')) as LocaleRegistry).locales.filter(locale=>locale.status==='published');
// build-seo.ts refuses to build unless the default language is published at "/".
const localeOf = (path: string): Locale => locales.filter(item=>item.prefix!=='/').find(item=>path.startsWith(item.prefix)) || locales.find(item=>item.prefix==='/')!;
const get = async (path: string): Promise<{ response: Response; text: string }> => {
  const response = await fetch(origin+path,{redirect:'manual',signal:AbortSignal.timeout(15000)});
  return {response,text:await response.text()};
};
const version = await get('/site-version.json');
assert.equal(version.response.status,200,'Expected deployed site-version.json; deploy first');
assert.equal(JSON.parse(version.text).hash,manifest.hash,'Production differs from this build');
for (const path of manifest.pages) {
  const {response,text} = await get(path);
  assert.equal(response.status,200,path);
  assert.match(response.headers.get('content-type')||'',/text\/html/,path);
  assert.doesNotMatch(response.headers.get('x-robots-tag')||'',/noindex/);
  assert.ok(text.includes(`rel="canonical" href="${origin}${path}"`),`Canonical ${path}`);
  assert.equal((text.match(/<h1(?:\s|>)/g)||[]).length,1,`One H1 ${path}`);
  const locale = localeOf(path);
  assert.ok(text.includes(`<html lang="${locale.lang}"`),`html lang ${path}`);
  assert.ok(text.includes(`hreflang="${locale.hreflang}" href="${origin}${path}"`),`hreflang ${path}`);
  if (locale.prefix==='/') assert.match(text,/준비 중|준비하고/);
  const visible = text.replace(/<script\b[^>]*>[\s\S]*?<\/script>/g,'').replace(/<style\b[^>]*>[\s\S]*?<\/style>/g,'').replace(/<[^>]*>/g,'');
  assert.doesNotMatch(visible,/LIMBO|수수료|₩|\d+\s*%/i);
  const schema = text.match(/<script type="application\/ld\+json">([\s\S]*?)<\/script>/);
  assert.ok(schema,`Schema ${path}`);
  JSON.parse(schema[1]);
  console.log(`OK ${origin}${path} (${locale.lang}) — readable without JavaScript`);
}
for (const path of ['/robots.txt','/sitemap.xml','/llms.txt','/llms-full.txt','/feed.xml']) {
  assert.equal((await get(path)).response.status,200,path);
}
const sitemap = (await get('/sitemap.xml')).text;
assert.equal((sitemap.match(/<loc>/g)||[]).length,manifest.pages.length);
for (const path of manifest.pages) assert.ok(sitemap.includes(`<loc>${origin}${path}</loc>`));
assert.equal((await get('/this-page-does-not-exist')).response.status,404);
for (const path of ['/publishing','/publishing/index.html','/index.html']) {
  const {response} = await get(path);
  assert.equal(response.status,308,path);
  assert.equal(response.headers.get('location'),origin+(path==='/index.html'?'/':'/publishing/'));
}
console.log(`Live verification passed: ${manifest.hash}`);
