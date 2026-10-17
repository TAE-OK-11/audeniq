#!/usr/bin/env node
// Cloudflare D1(audeniq-content: 공지·이벤트·점검) → Postgres(content 스키마, 마이그레이션 0086) 옮기기
//
//   CLOUDFLARE_API_TOKEN=... CLOUDFLARE_ACCOUNT_ID=... node --experimental-strip-types deploy/aws/migrate-d1.ts > d1-import.sql
//   psql "$OWNER_DATABASE_URL" -v ON_ERROR_STOP=1 -f d1-import.sql
//
// 같은 id가 이미 있으면 건너뛴다(ON CONFLICT DO NOTHING) — 여러 번 실행해도 된다.
// 전환 직전(관리 화면에서 글을 더 쓰지 않을 때) 한 번 더 실행해 마지막 변경까지 옮긴다.
// 설문(audeniq-survey)은 베타 전까지 D1 Worker 그대로라 옮기지 않는다.
import { execFileSync } from 'node:child_process';
import { randomBytes } from 'node:crypto';

interface Table {
  db: string;
  table: string;
  /** jsonb_to_recordset 의 열 정의 */
  columns: string;
  insert: string;
}

/** wrangler d1 execute --json 결과 (한 문장이면 배열이 아닐 수 있다) */
interface D1Result {
  results?: Record<string, unknown>[];
}

const TABLES: Table[] = [
  {
    db: 'audeniq-content', table: 'notices',
    columns: 'id text, title text, body text, pinned int, published_at text, created_at text, updated_at text, deleted_at text',
    insert: `INSERT INTO content.notices (id,title,body,pinned,published_at,created_at,updated_at,deleted_at)
  SELECT id,title,body,pinned=1,published_at::timestamptz,created_at::timestamptz,updated_at::timestamptz,deleted_at::timestamptz`,
  },
  {
    db: 'audeniq-content', table: 'events',
    columns: 'id text, title text, summary text, body text, place text, starts_on text, ends_on text, link_url text, published_at text, created_at text, updated_at text, deleted_at text',
    insert: `INSERT INTO content.events (id,title,summary,body,place,starts_on,ends_on,link_url,published_at,created_at,updated_at,deleted_at)
  SELECT id,title,summary,body,place,starts_on::date,NULLIF(ends_on,'')::date,NULLIF(link_url,''),published_at::timestamptz,created_at::timestamptz,updated_at::timestamptz,deleted_at::timestamptz`,
  },
  {
    db: 'audeniq-content', table: 'maintenance',
    columns: 'id text, title text, body text, starts_at text, ends_at text, kind text, end_unknown int, published_at text, created_at text, updated_at text, deleted_at text',
    insert: `INSERT INTO content.maintenance (id,title,body,starts_at,ends_at,kind,end_unknown,published_at,created_at,updated_at,deleted_at)
  SELECT id,title,body,starts_at::timestamptz,ends_at::timestamptz,kind,end_unknown=1,published_at::timestamptz,created_at::timestamptz,updated_at::timestamptz,deleted_at::timestamptz`,
  },
];

function rows(db: string, table: string): Record<string, unknown>[] {
  const out = execFileSync('npx', ['--yes', 'wrangler@4.141.0', 'd1', 'execute', db, '--remote', '--json',
    '--command', `SELECT * FROM ${table}`], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'], maxBuffer: 1 << 30 });
  const parsed: D1Result | D1Result[] = JSON.parse(out);
  return (Array.isArray(parsed) ? parsed : [parsed]).flatMap(r => r.results ?? []);
}

/** jsonb_to_recordset 으로 넣는 SQL — 값은 JSON 한 덩어리로, 따옴표 이스케이프 없이 달러 인용 */
export function importSql({ table, columns, insert }: Table, data: unknown[]): string {
  const json = JSON.stringify(data);
  let tag: string;
  do tag = `d1_${randomBytes(6).toString('hex')}`; while (json.includes(`$${tag}$`));
  return `-- ${table}: ${data.length}\n${insert}\n  FROM jsonb_to_recordset($${tag}$${json}$${tag}$::jsonb) AS x(${columns})\n  ON CONFLICT (id) DO NOTHING;\n`;
}

if (process.argv[1] && import.meta.url.endsWith(process.argv[1].split('/').pop()!)) {
  const parts = ['BEGIN;'];
  for (const t of TABLES) {
    const data = rows(t.db, t.table);
    console.error(`${t.db}.${t.table}: ${data.length}`);
    parts.push(importSql(t, data));
  }
  parts.push('COMMIT;');
  console.log(parts.join('\n'));
}

export { TABLES };
