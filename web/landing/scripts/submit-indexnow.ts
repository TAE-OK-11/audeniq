// Run manually AFTER deployment. No request is sent by build/check/commit.
//   npm run indexnow                       submit every published page
//   npm run indexnow -- --since <git-ref>  submit only pages added, changed or removed since <git-ref>
//   npm run indexnow -- --dry-run          verify and print the URL list without submitting
// INDEXNOW_SINCE may replace --since (used by the GitHub Actions workflow input).
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import type { SiteManifest } from './content-types.ts';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const read = (name: string): string => readFileSync(join(root,name),'utf8');
const args = process.argv.slice(2);
const since = (args.includes('--since') ? args[args.indexOf('--since') + 1] : process.env.INDEXNOW_SINCE || '').trim();
const dryRun = args.includes('--dry-run');
const key = read('content/indexnow-key.txt').trim();
const manifest: SiteManifest = JSON.parse(read('content/generated-manifest.json'));
const origin = 'https://audeniq.com';
assert.match(key,/^[a-f0-9]{32}$/);
const keyLocation = `${origin}/${key}.txt`;

// Changed pages: compare per-page content hashes with the manifest at <since>.
let changed = manifest.pages, removed: string[] = [];
if (since) {
  assert.match(since, /^[\w./~^-]+$/, 'Invalid git ref');
  const previous: Partial<SiteManifest> = JSON.parse(execFileSync('git', ['show', `${since}:web/landing/content/generated-manifest.json`], { cwd: root, encoding: 'utf8' }));
  const before = previous.pageHashes || {};
  changed = manifest.pages.filter(path => !before[path] || before[path] !== manifest.pageHashes[path]);
  removed = (previous.pages || []).filter(path => !manifest.pages.includes(path));
  console.log(`Since ${since}: ${changed.length} new/changed, ${removed.length} removed page(s).`);
}
if (!changed.length && !removed.length) { console.log('No published page changed; nothing to submit.'); process.exit(0); }

const get = (url: string): Promise<Response> => fetch(url,{redirect:'manual',signal:AbortSignal.timeout(15000)});
const version = await get(`${origin}/site-version.json`);
assert.equal(version.status,200,'Deploy this build before submitting');
assert.equal((await version.json()).hash,manifest.hash,'Production differs from this build');
const ownership = await get(keyLocation);
assert.equal(ownership.status,200,'Public IndexNow key is not deployed');
assert.equal((await ownership.text()).trim(),key);
for (const path of changed) {
  const response = await get(origin + path);
  await response.body?.cancel();
  assert.equal(response.status,200,`Not a published page: ${path}`);
  assert.doesNotMatch(response.headers.get('x-robots-tag')||'',/noindex/);
}
for (const path of removed) {
  const response = await get(origin + path);
  await response.body?.cancel();
  assert.ok([301,308,404,410].includes(response.status),`Removed page still answers ${response.status}: ${path}`);
}
const urlList = [...changed, ...removed].map(path=>origin+path);
if (dryRun) { console.log(urlList.join('\n')); process.exit(0); }
const response = await fetch('https://api.indexnow.org/indexnow',{
  method:'POST',headers:{'Content-Type':'application/json; charset=utf-8'},
  body:JSON.stringify({host:'audeniq.com',key,keyLocation,urlList}),
  signal:AbortSignal.timeout(20000)
});
assert.ok([200,202].includes(response.status),`IndexNow HTTP ${response.status}: ${await response.text()}`);
console.log(`IndexNow accepted ${urlList.length} URL(s) (HTTP ${response.status}); indexing is decided by search engines.`);
