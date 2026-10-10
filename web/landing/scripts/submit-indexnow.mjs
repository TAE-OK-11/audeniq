// Run manually AFTER deployment. No request is sent by build/check/commit.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const read = name => readFileSync(join(root,name),'utf8');
const key = read('content/indexnow-key.txt').trim();
const manifest = JSON.parse(read('content/generated-manifest.json'));
const origin = 'https://audeniq.com';
assert.match(key,/^[a-f0-9]{32}$/);
const keyLocation = `${origin}/${key}.txt`;
const get = url => fetch(url,{redirect:'manual',signal:AbortSignal.timeout(15000)});
const version = await get(`${origin}/site-version.json`);
assert.equal(version.status,200,'Deploy this build before submitting');
assert.equal((await version.json()).hash,manifest.hash,'Production differs from this build');
const ownership = await get(keyLocation);
assert.equal(ownership.status,200,'Public IndexNow key is not deployed');
assert.equal((await ownership.text()).trim(),key);
const urlList = manifest.pages.map(path=>origin+path);
for (const url of urlList) {
  const response = await get(url);
  await response.body?.cancel();
  assert.equal(response.status,200,`Not a published page: ${url}`);
  assert.doesNotMatch(response.headers.get('x-robots-tag')||'',/noindex/);
}
const response = await fetch('https://api.indexnow.org/indexnow',{
  method:'POST',headers:{'Content-Type':'application/json'},
  body:JSON.stringify({host:'audeniq.com',key,keyLocation,urlList}),
  signal:AbortSignal.timeout(20000)
});
assert.ok([200,202].includes(response.status),`IndexNow HTTP ${response.status}: ${await response.text()}`);
console.log(`IndexNow accepted ${urlList.length} public URLs (HTTP ${response.status}); indexing is decided by search engines.`);
