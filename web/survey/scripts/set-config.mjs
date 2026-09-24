import { readFileSync, writeFileSync } from 'node:fs';
const [databaseId, siteKey] = process.argv.slice(2);
const isUuid = /^[a-f\d]{8}-[a-f\d]{4}-[a-f\d]{4}-[a-f\d]{4}-[a-f\d]{12}$/i;
if (!isUuid.test(databaseId ?? '') || !siteKey || siteKey.includes('REPLACE') || siteKey.length < 6) {
  console.error('사용법: bun scripts/set-config.mjs <D1_database_id> <Turnstile_공개_Site_Key>');
  console.error('주의: Turnstile 비밀 Secret Key는 절대 이 명령어에 넣지 마세요.');
  process.exit(1);
}
const path = new URL('../wrangler.jsonc', import.meta.url);
const cfg = JSON.parse(readFileSync(path, 'utf8'));
const db = cfg.d1_databases?.find(item => item.binding === 'DB');
if (!db) throw new Error('D1 바인딩 DB를 찾지 못했습니다.');
db.database_id = databaseId;
cfg.vars ??= {};
cfg.vars.TURNSTILE_SITE_KEY = siteKey;
writeFileSync(path, `${JSON.stringify(cfg, null, 2)}\n`);
const htmlPath = new URL('../public/index.html', import.meta.url);
const html = readFileSync(htmlPath, 'utf8');
const meta = /<meta name="turnstile-site-key" content="[^"]*">/;
if (!meta.test(html)) throw new Error('index.html의 Turnstile 공개 키 메타태그가 없습니다.');
writeFileSync(htmlPath, html.replace(meta, `<meta name="turnstile-site-key" content="${siteKey}">`));
console.log('wrangler.jsonc 및 정적 index.html에 D1 ID와 공개 Site Key를 적용했습니다.');
console.log('비밀 키는 파일에 쓰지 않았습니다.');
