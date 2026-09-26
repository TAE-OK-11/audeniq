import test from 'node:test';
import assert from 'node:assert/strict';
import worker, { bearerOk, eventStatus, route, validate } from '../worker.js';

const TOKEN = 'x'.repeat(40);

/** 메모리 D1 대역 — worker.js가 쓰는 prepare().bind().all/first/run만 흉내 */
function fakeDb() {
  const rows = { notices: [], events: [] };
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
  assert.equal((await call(e, 'GET', '/api/unknown')).status, 404);
  assert.equal(await (await call(e, 'GET', '/notices/abc')).text(), 'asset');
});
