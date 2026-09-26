import test from 'node:test';
import assert from 'node:assert/strict';
import worker, { bearerOk, eventStatus, pickStatus, resetMaintenanceCache, route, validate } from '../worker.js';

const TOKEN = 'x'.repeat(40);

/** 메모리 D1 대역 — worker.js가 쓰는 prepare().bind().all/first/run만 흉내 */
function fakeDb() {
  const rows = { notices: [], events: [], maintenance: [] };
  const exec = (sql, args) => {
    const table = /FROM (\w+)|INTO (\w+)|UPDATE (\w+)/.exec(sql).slice(1).find(Boolean);
    const list = rows[table];
    if (sql.startsWith('INSERT')) {
      const cols = /\(([^)]+)\) VALUES/.exec(sql)[1].split(', ');
      if (list.some(r => r.id === args[0])) throw new Error('UNIQUE constraint failed');
      const row = { deleted_at: null, created_at: 'now', updated_at: 'now' };
      cols.forEach((c, i) => { row[c] = args[i]; });
      list.push(row);
      return { changes: 1 };
    }
    if (sql.startsWith('UPDATE') && sql.includes('SET deleted_at')) {
      const r = list.find(x => x.id === args[1] && !x.deleted_at);
      if (r) r.deleted_at = args[0];
      return { changes: r ? 1 : 0 };
    }
    if (sql.startsWith('UPDATE')) {
      const cols = [...sql.matchAll(/(\w+) = \?(\d+)/g)].map(m => [m[1], Number(m[2]) - 1]);
      const id = args[args.length - 1];
      const r = list.find(x => x.id === id);
      if (r) { for (const [c, i] of cols) if (c !== 'id') r[c] = args[i]; r.deleted_at = null; }
      return { changes: r ? 1 : 0 };
    }
    let out = list;
    if (sql.includes('WHERE id = ?1')) out = out.filter(r => r.id === args[0]);
    if (sql.includes('deleted_at IS NULL')) out = out.filter(r => !r.deleted_at);
    if (sql.includes('published_at <= ?')) {
      const now = sql.includes('?2') ? args[1] : args[0];
      out = out.filter(r => r.published_at <= now);
    }
    return { results: out.map(r => ({ ...r })) };
  };
  return {
    rows,
    prepare(sql) {
      let args = [];
      const stmt = {
        bind(...a) { args = a; return stmt; },
        async all() { return { results: exec(sql, args).results }; },
        async first() { return exec(sql, args).results?.[0] ?? null; },
        async run() { return { meta: exec(sql, args) }; },
      };
      return stmt;
    },
  };
}

const env = () => ({ CONTENT_DB: fakeDb(), CONTENT_ADMIN_TOKEN: TOKEN, ASSETS: { fetch: async () => new Response('asset') } });
const call = (e, method, path, body, token = TOKEN) => worker.fetch(new Request(`https://studio.audeniq.com${path}`, {
  method,
  headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' },
  body: body ? JSON.stringify(body) : undefined,
}), e);

test('routes: public reads, admin writes under /api/content', () => {
  assert.deepEqual(route('GET', '/api/notices'), { kind: 'list', table: 'notices' });
  assert.deepEqual(route('GET', '/api/events/a-1'), { kind: 'get', table: 'events', id: 'a-1' });
  assert.deepEqual(route('POST', '/api/notices'), { kind: 'method' });
  assert.deepEqual(route('GET', '/api/content/notices'), { kind: 'adminList', table: 'notices' });
  assert.deepEqual(route('PUT', '/api/content/notices/n1'), { kind: 'update', table: 'notices', id: 'n1' });
  assert.deepEqual(route('DELETE', '/api/content/events/e1'), { kind: 'delete', table: 'events', id: 'e1' });
  assert.equal(route('GET', '/api/notices/Bad_ID'), null);
  assert.equal(route('GET', '/api/orgs/x'), null);
  assert.equal(route('GET', '/notices'), null);
});

test('validation mirrors the D1 schema', () => {
  assert.throws(() => validate('notices', { title: ' ', body: '' }), { code: 'TITLE_REQUIRED' });
  assert.throws(() => validate('notices', { title: 'a', body: '', extra: 1 }), { code: 'INVALID_INPUT' });
  assert.throws(() => validate('notices', { title: 'a‮b', body: '' }), { code: 'TEXT_INVALID_CHARACTERS' });
  assert.throws(() => validate('notices', { title: 'a', body: '', published_at: '2026-09-26' }), { code: 'PUBLISHED_AT_INVALID' });
  assert.equal(validate('notices', { title: 'a', body: '줄\n줄', pinned: true }).values[2], 1);
  assert.throws(() => validate('events', { title: 'a', body: '', starts_on: '2026-10-02', ends_on: '2026-10-01' }), { code: 'DATES_INVALID' });
  assert.throws(() => validate('events', { title: 'a', body: '', starts_on: '2026-10-02', link_url: 'http://x' }), { code: 'LINK_URL_INVALID' });
});

test('bearer token and event status', () => {
  assert.equal(bearerOk(`Bearer ${TOKEN}`, TOKEN), true);
  assert.equal(bearerOk('Bearer short', 'short'), false);
  assert.equal(bearerOk(`Bearer ${'y'.repeat(40)}`, TOKEN), false);
  assert.equal(eventStatus('2026-10-01', '2026-10-31', '2026-09-30'), 'upcoming');
  assert.equal(eventStatus('2026-10-01', null, '2026-10-01'), 'ongoing');
  assert.equal(eventStatus('2026-10-01', '2026-10-31', '2026-11-01'), 'ended');
});

test('posting a notice makes it public; scheduled and removed ones stay hidden', async () => {
  const e = env();
  assert.equal((await call(e, 'POST', '/api/content/notices', { title: '공지', body: 'b' }, 'nope')).status, 401);
  const created = await (await call(e, 'POST', '/api/content/notices', { id: 'hello', title: '공지', body: '본문', pinned: true })).json();
  assert.equal(created.pinned, true);
  assert.equal((await call(e, 'POST', '/api/content/notices', { id: 'hello', title: 'dup', body: '' })).status, 409);
  await call(e, 'POST', '/api/content/notices', { title: '예약', body: '', published_at: '2999-01-01T00:00:00Z' });

  let pub = await (await call(e, 'GET', '/api/notices')).json();
  assert.deepEqual(pub.items.map(n => n.id), ['hello']);
  assert.equal((await call(e, 'GET', '/api/notices/hello')).status, 200);

  const admin = await (await call(e, 'GET', '/api/content/notices')).json();
  assert.equal(admin.items.length, 2);

  await call(e, 'PUT', '/api/content/notices/hello', { title: '고친 공지', body: '본문', pinned: false, published_at: '2026-01-01T00:00:00Z' });
  assert.equal((await (await call(e, 'GET', '/api/notices/hello')).json()).title, '고친 공지');

  assert.equal((await call(e, 'DELETE', '/api/content/notices/hello')).status, 200);
  pub = await (await call(e, 'GET', '/api/notices')).json();
  assert.equal(pub.items.length, 0);
  assert.equal((await call(e, 'GET', '/api/notices/hello')).status, 404);

  // 다시 저장하면 복구
  await call(e, 'PUT', '/api/content/notices/hello', { title: '복구', body: '', published_at: '2026-01-01T00:00:00Z' });
  assert.equal((await (await call(e, 'GET', '/api/notices')).json()).items[0].title, '복구');
});

test('admin API is off without a token, other paths go to assets', async () => {
  const e = { ...env(), CONTENT_ADMIN_TOKEN: undefined };
  assert.equal((await call(e, 'GET', '/api/content/notices')).status, 503);
  assert.equal(await (await call(e, 'GET', '/notices/abc')).text(), 'asset');
});

test('screen paths fall back to index.html when assets answer 404', async () => {
  const assets = { fetch: async req => {
    const p = new URL(req.url).pathname;
    return p === '/' ? new Response('<html>', { headers: { 'Content-Security-Policy': "default-src 'self'" } }) : new Response('nf', { status: 404 });
  } };
  const e = { ...env(), ASSETS: assets };
  const r = await call(e, 'GET', '/notices/abc');
  assert.equal(r.status, 200);
  assert.equal(await r.text(), '<html>');
  assert.equal(r.headers.get('Content-Security-Policy'), "default-src 'self'");
  assert.equal(r.headers.get('Cache-Control'), 'no-cache');
  assert.equal((await call(e, 'GET', '/missing.js')).status, 404);
});

test('maintenance: admin schedules it, /api/status shows active and upcoming', async () => {
  const e = env();
  e.CONTENT_DB.rows.maintenance = [];
  const iso = ms => new Date(ms).toISOString().replace(/\.\d{3}Z$/, 'Z');
  const now = Date.now();
  assert.equal((await call(e, 'POST', '/api/content/maintenance', { title: '점검', starts_at: iso(now + 3600e3), ends_at: iso(now) })).status, 400);
  assert.equal((await call(e, 'POST', '/api/content/maintenance', { title: '다음 점검', body: 'DB 업그레이드', starts_at: iso(now + 3600e3), ends_at: iso(now + 7200e3) })).status, 201);
  let st = await (await call(e, 'GET', '/api/status')).json();
  assert.equal(st.maintenance.active, null);
  assert.equal(st.maintenance.upcoming.title, '다음 점검');
  await call(e, 'POST', '/api/content/maintenance', { id: 'now', title: '지금 점검', starts_at: iso(now - 60e3), ends_at: iso(now + 600e3) });
  st = await (await call(e, 'GET', '/api/status')).json();
  assert.equal(st.maintenance.active.id, 'now');
  // 공개 목록 경로는 없다
  assert.equal(route('GET', '/api/maintenance'), null);
  // 끝나면 사라진다
  await call(e, 'DELETE', '/api/content/maintenance/now');
  st = await (await call(e, 'GET', '/api/status')).json();
  assert.equal(st.maintenance.active, null);
});

test('status answers even without the maintenance table', async () => {
  const e = { ...env(), CONTENT_DB: undefined };
  const st = await (await call(e, 'GET', '/api/status')).json();
  assert.deepEqual(st.maintenance, { active: null, upcoming: null });
});

test('pickStatus ignores unpublished and far-future windows', () => {
  const now = '2026-09-26T00:00:00Z';
  const rows = [
    { id: 'a', starts_at: '2026-09-30T00:00:00Z', ends_at: '2026-09-30T02:00:00Z', published_at: '2026-09-01T00:00:00Z' },
    { id: 'b', starts_at: '2026-09-26T05:00:00Z', ends_at: '2026-09-26T06:00:00Z', published_at: '2026-09-27T00:00:00Z' },
  ];
  assert.deepEqual(pickStatus(rows, now).maintenance, { active: null, upcoming: null });
});

test('other /api/* calls go to the backend with the service header; a dead backend is 502', async () => {
  const seen = [];
  const real = globalThis.fetch;
  try {
    globalThis.fetch = async req => { seen.push(req); return new Response('{"ok":true}', { status: 200 }); };
    const e = { ...env(), EDGE_SERVICE_SECRET: 's'.repeat(40) };
    const r = await call(e, 'GET', '/api/me?x=1');
    assert.equal(r.status, 200);
    assert.match(seen[0].url, /\/api\/me\?x=1$/);
    assert.equal(seen[0].headers.get('x-audeniq-service'), 's'.repeat(40));
    globalThis.fetch = async () => { throw new Error('down'); };
    const down = await call(e, 'GET', '/api/me');
    assert.equal(down.status, 502);
    assert.equal((await down.json()).error.code, 'BACKEND_UNAVAILABLE');
  } finally {
    globalThis.fetch = real;
  }
});

test('emergency: env switch forces maintenance and API calls get 503 MAINTENANCE', async () => {
  resetMaintenanceCache();
  const real = globalThis.fetch;
  let proxied = 0;
  globalThis.fetch = async () => { proxied++; return new Response('{}'); };
  try {
    const e = { ...env(), MAINTENANCE_MODE: 'on', MAINTENANCE_MESSAGE: 'DB 복구 중이에요.' };
    const st = await (await call(e, 'GET', '/api/status')).json();
    assert.equal(st.maintenance.active.kind, 'emergency');
    assert.equal(st.maintenance.active.end_unknown, true);
    assert.equal(st.maintenance.active.body, 'DB 복구 중이에요.');
    const blocked = await call(e, 'POST', '/api/releases', { a: 1 });
    assert.equal(blocked.status, 503);
    assert.equal((await blocked.json()).error.code, 'MAINTENANCE');
    assert.equal(proxied, 0);
    // 공지·관리 API와 화면은 그대로
    assert.equal((await call(e, 'GET', '/api/notices')).status, 200);
    // 끄면 다시 통과
    const off = { ...e, MAINTENANCE_MODE: 'off' };
    resetMaintenanceCache();
    assert.equal((await call(off, 'GET', '/api/me')).status, 200);
    assert.equal(proxied, 1);
  } finally {
    globalThis.fetch = real;
  }
});

test('emergency window from the admin API blocks the API until it ends', async () => {
  resetMaintenanceCache();
  const real = globalThis.fetch;
  globalThis.fetch = async () => new Response('{}');
  try {
    const e = env();
    const iso = ms => new Date(ms).toISOString().replace(/\.\d{3}Z$/, 'Z');
    const created = await (await call(e, 'POST', '/api/content/maintenance', {
      id: 'urgent', title: '긴급 점검', kind: 'emergency', end_unknown: true,
      starts_at: iso(Date.now() - 1000), ends_at: iso(Date.now() + 3600e3),
    })).json();
    assert.equal(created.kind, 'emergency');
    assert.equal(created.end_unknown, true);
    assert.equal((await call(e, 'GET', '/api/me')).status, 503);
    // 종료: ends_at을 지금으로
    const res = await call(e, 'PUT', '/api/content/maintenance/urgent', {
      title: '긴급 점검', kind: 'emergency', starts_at: created.starts_at, ends_at: iso(Date.now()),
    });
    assert.equal(res.status, 200);
    assert.equal((await call(e, 'GET', '/api/me')).status, 200);
    assert.throws(() => validate('maintenance', { title: 'x', starts_at: created.starts_at, ends_at: iso(Date.now() + 1e6), kind: 'soon' }), { code: 'INVALID_INPUT' });
  } finally {
    globalThis.fetch = real;
  }
});
