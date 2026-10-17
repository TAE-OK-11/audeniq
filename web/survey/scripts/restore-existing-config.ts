import { readFileSync, writeFileSync } from 'node:fs';
/** wrangler.jsonc 중 이 스크립트가 읽고 쓰는 부분 */
interface D1Binding { binding: string; database_name?: string; database_id?: string; migrations_dir?: string }
interface WranglerConfig {
  name?: string;
  workers_dev?: boolean;
  routes?: { pattern: string; custom_domain?: boolean }[];
  /** 현재 설정에는 항상 있다고 보고 바로 덮어쓴다 (없으면 TypeError) */
  assets: { run_worker_first?: boolean | string[]; html_handling?: string; not_found_handling?: string };
  vars?: { TURNSTILE_SITE_KEY?: string };
  d1_databases?: D1Binding[];
}

// Read the operator's own backed-up local config; never prints or exposes secrets.
const backup = process.argv[2];
if (!backup) {
  console.error('사용법: bun scripts/restore-existing-config.ts wrangler.jsonc.before-opt');
  process.exit(1);
}
const prior: WranglerConfig = JSON.parse(readFileSync(backup, 'utf8'));
const oldDatabase = prior.d1_databases?.find(item => item.binding === 'DB' || item.database_name === 'audeniq-survey');
const id = oldDatabase?.database_id;
const key = prior.vars?.TURNSTILE_SITE_KEY;
if (!/^[a-f0-9]{8}(-[a-f0-9]{4}){3}-[a-f0-9]{12}$/i.test(id || '') || !key || key.includes('REPLACE')) {
  console.error('이전 설정에 실제 D1 database_id 또는 Turnstile 공개 키가 없습니다. 기존 설정 백업을 확인해 주세요.');
  process.exit(1);
}
const cfgPath = new URL('../wrangler.jsonc', import.meta.url);
const current: WranglerConfig = JSON.parse(readFileSync(cfgPath, 'utf8'));
if (current.name !== 'audeniq-survey') throw new Error('작업 대상이 AUDENIQ 설문 Worker가 아닙니다.');
current.d1_databases = [{binding:'DB',database_name:'audeniq-survey',database_id:id,migrations_dir:'migrations'}];
current.vars ??= {};
current.vars.TURNSTILE_SITE_KEY = key;
// Keep the new selective routing. Do not copy the old run_worker_first=true.
current.assets.run_worker_first = ['/api/*','/health'];
current.assets.html_handling = 'auto-trailing-slash';
current.assets.not_found_handling = '404-page';
writeFileSync(cfgPath, JSON.stringify(current,null,2)+'\n');
const htmlPath = new URL('../public/index.html',import.meta.url);
const html = readFileSync(htmlPath,'utf8');
const meta = /<meta name="turnstile-site-key" content="[^"]*">/;
if (!meta.test(html)) throw new Error('정적 HTML에 Turnstile 메타태그가 없습니다.');
writeFileSync(htmlPath, html.replace(meta, `<meta name="turnstile-site-key" content="${key}">`));
console.log('기존 D1 ID·공개 키를 유지하고 새 정적 라우팅·HTML에 적용했어요. 비밀 키는 읽거나 출력하지 않았습니다.');
