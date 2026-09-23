import { readFileSync } from 'node:fs';

// Wrangler config is JSONC; this generated file is plain JSON so native parse is safe.
const config = JSON.parse(readFileSync(new URL('../wrangler.jsonc', import.meta.url), 'utf8'));
const database = config.d1_databases?.find(item => item.binding === 'DB');
const errors = [];
if (config.name !== 'audeniq-survey') errors.push('Worker 이름을 audeniq-survey로 유지해 주세요.');
if (JSON.stringify(config.assets?.run_worker_first)!==JSON.stringify(['/api/*','/health'])) errors.push('Worker는 /api/* 및 /health에서만 우선 실행해야 해요.');
if (config.assets?.html_handling!=='auto-trailing-slash'||config.assets?.not_found_handling!=='404-page') errors.push('정적 HTML/404 라우팅 설정을 확인해 주세요.');
if (!config.workers_dev && !config.routes?.some(r => r.pattern === 'survey.audeniq.com' && r.custom_domain === true)) errors.push('설문 서브도메인 라우팅을 확인해 주세요.');
if (!database || !/^[a-f\d]{8}-[a-f\d]{4}-[a-f\d]{4}-[a-f\d]{4}-[a-f\d]{12}$/i.test(database.database_id ?? '')) errors.push('wrangler.jsonc에 실제 D1 database_id를 입력해 주세요.');
if (!config.vars?.TURNSTILE_SITE_KEY || /REPLACE|YOUR_|TEST_PLACEHOLDER/i.test(config.vars.TURNSTILE_SITE_KEY)) errors.push('wrangler.jsonc의 TURNSTILE_SITE_KEY를 실제 사이트 키로 바꿔 주세요.');
const html = readFileSync(new URL('../public/index.html',import.meta.url),'utf8');
const staticKey = html.match(/<meta name="turnstile-site-key" content="([^"]*)">/)?.[1];
if (staticKey !== config.vars?.TURNSTILE_SITE_KEY) errors.push('정적 HTML 공개 Site Key와 Worker 설정이 불일치합니다. set-config를 다시 실행해 주세요.');
if (config.routes?.some(r => r.pattern === 'audeniq.com' || r.pattern === 'studio.audeniq.com')) errors.push('홍보 홈페이지/스튜디오 도메인을 설문 Worker에 연결하면 안 돼요.');
if (errors.length) { for(const e of errors) console.error('설정 확인 필요:', e); process.exit(1); }
console.log('설정 검사 통과: Worker 이름, 설문 도메인, D1 binding, Turnstile 사이트 키');
console.log('참고: TURNSTILE_SECRET_KEY 실제 등록 여부는 Cloudflare 배포 후 /api/config 에서 확인해 주세요.');
