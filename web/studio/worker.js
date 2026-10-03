/**
 * AUDENIQ STUDIO Worker
 * - 정적 에셋(React 빌드)은 Workers Static Assets가 서빙
 * - 공지·이벤트는 D1(CONTENT_DB)에서 서빙
 *
 * 공개 (로그인 없이 읽기)
 *   GET  /api/notices, /api/notices/:id
 *   GET  /api/events,  /api/events/:id
 * 관리 (ADMIN 세션 + CSRF; 비상 스크립트는 CONTENT_ADMIN_TOKEN)
 *   GET    /api/content/notices|events        예약·숨김 글까지 전체 목록
 *   POST   /api/content/notices|events        새 글
 *   PUT    /api/content/notices|events/:id    수정
 *   DELETE /api/content/notices|events/:id    삭제 (deleted_at 기록, 복구 가능)
 *   (서버 점검 일정도 같은 방식: /api/content/maintenance[/:id])
 * 상태
 *   GET  /api/status   진행 중·예정된 서버 점검 (스튜디오의 점검 화면·예고 배너)
 * 점검 중 차단
 *   점검 중에는 일반 백엔드 API를 503 MAINTENANCE로 막는다. 관리 세션 확인·로그인은 유지한다.
 * 비상 스위치 (D1·관리 화면이 안 될 때)
 *   켜기: echo on | npx wrangler secret put MAINTENANCE_MODE   (선택: MAINTENANCE_MESSAGE, MAINTENANCE_UNTIL=ISO 시각)
 *   끄기: npx wrangler secret delete MAINTENANCE_MODE
 *
 * 경로·입력 규칙은 crates/edge/src/content.rs(Rust 엣지)와 같다.
 */

import { backendOrigin, clientEncoding, passThroughEncoding, readLimitedBody, requireHttps, secureResponse, serveAssets } from '../shared/transport.js';
export { passThroughEncoding } from '../shared/transport.js';

const TABLES = new Set(['notices', 'events', 'maintenance']);
// 공개 목록·상세가 있는 표 (maintenance는 /api/status로만 공개)
const PUBLIC_TABLES = new Set(['notices', 'events']);
const NOTICE_COLUMNS = 'id, title, body, pinned, published_at, updated_at';
const EVENT_COLUMNS = 'id, title, summary, body, place, starts_on, ends_on, link_url, published_at, updated_at';
const MAINTENANCE_COLUMNS = 'id, title, body, starts_at, ends_at, kind, end_unknown, published_at, updated_at';
// 예고 배너는 시작 72시간 전부터
const NOTICE_AHEAD_MS = 72 * 3_600_000;
const PUBLIC_CACHE = 'public, max-age=30';
const MAX_BODY = 64 * 1024;

export const ID_RE = /^[a-z0-9-]{1,64}$/;
const DATE_RE = /^\d{4}-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])$/;
const TS_RE = /^\d{4}-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])T([01]\d|2[0-3]):[0-5]\d:[0-5]\dZ$/;
// 제어문자(줄바꿈·탭 제외)와 보이지 않는 방향 제어 문자
const BAD_TEXT = /[\u0000-\u0008\u000b-\u001f\u007f-\u009f​-‏‪-‮⁦-⁩﻿]/;
const BAD_LINE = /[\u0000-\u001f\u007f-\u009f​-‏‪-‮⁦-⁩﻿]/;

const nowUtc = () => new Date().toISOString().replace(/\.\d{3}Z$/, 'Z');
export const todayKst = (ms = Date.now()) => new Date(ms + 9 * 3_600_000).toISOString().slice(0, 10);

export function eventStatus(startsOn, endsOn, today = todayKst()) {
  if (today < startsOn) return 'upcoming';
  if (today <= (endsOn || startsOn)) return 'ongoing';
  return 'ended';
}

/** 공개 목록·상세 응답 모양으로 (pinned는 불리언, 이벤트는 진행 상태 포함) */
export function present(table, row, today = todayKst()) {
  if (table === 'notices') return { ...row, pinned: row.pinned === 1 || row.pinned === true };
  if (table === 'maintenance') return { ...row, kind: row.kind === 'emergency' ? 'emergency' : 'scheduled', end_unknown: row.end_unknown === 1 || row.end_unknown === true };
  return { ...row, status: eventStatus(row.starts_on, row.ends_on, today) };
}

class InputError extends Error {
  constructor(code) { super(code); this.code = code; }
}

function text(v, max, { multiline = false, required = false } = {}) {
  if (v == null) v = '';
  if (typeof v !== 'string') throw new InputError('INVALID_INPUT');
  const t = v.replace(/\r\n?/g, '\n').trim();
  if (required && !t) throw new InputError('TITLE_REQUIRED');
  if ([...t].length > max) throw new InputError('TOO_LONG');
  if ((multiline ? BAD_TEXT : BAD_LINE).test(t)) throw new InputError('TEXT_INVALID_CHARACTERS');
  return t;
}

function checkKeys(input, allowed) {
  if (!input || typeof input !== 'object' || Array.isArray(input)) throw new InputError('INVALID_INPUT');
  for (const k of Object.keys(input)) if (!allowed.includes(k)) throw new InputError('INVALID_INPUT');
}

function checkId(id) {
  if (id != null && id !== '' && (typeof id !== 'string' || !ID_RE.test(id))) throw new InputError('ID_INVALID');
  return id || null;
}

function publishedAt(v) {
  if (v == null || v === '') return nowUtc();
  if (typeof v !== 'string' || !TS_RE.test(v)) throw new InputError('PUBLISHED_AT_INVALID');
  return v;
}

/** 입력 검증 → [컬럼, 값] (INSERT/UPDATE 공통). 규칙은 D1 스키마의 CHECK와 같다. */
export function validate(table, input) {
  if (table === 'notices') {
    checkKeys(input, ['id', 'title', 'body', 'pinned', 'published_at']);
    if (input.pinned != null && typeof input.pinned !== 'boolean') throw new InputError('INVALID_INPUT');
    return {
      id: checkId(input.id),
      columns: ['title', 'body', 'pinned', 'published_at'],
      values: [
        text(input.title, 200, { required: true }),
        text(input.body, 20000, { multiline: true }),
        input.pinned ? 1 : 0,
        publishedAt(input.published_at),
      ],
    };
  }
  if (table === 'maintenance') {
    checkKeys(input, ['id', 'title', 'body', 'starts_at', 'ends_at', 'published_at', 'kind', 'end_unknown']);
    if (input.kind != null && input.kind !== 'scheduled' && input.kind !== 'emergency') throw new InputError('INVALID_INPUT');
    if (input.end_unknown != null && typeof input.end_unknown !== 'boolean') throw new InputError('INVALID_INPUT');
    const startsAt = input.starts_at;
    const endsAt = input.ends_at;
    if (typeof startsAt !== 'string' || !TS_RE.test(startsAt) || typeof endsAt !== 'string' || !TS_RE.test(endsAt) || endsAt <= startsAt) {
      throw new InputError('DATES_INVALID');
    }
    return {
      id: checkId(input.id),
      columns: ['title', 'body', 'starts_at', 'ends_at', 'kind', 'end_unknown', 'published_at'],
      values: [
        text(input.title, 200, { required: true }),
        text(input.body, 2000, { multiline: true }),
        startsAt,
        endsAt,
        input.kind === 'emergency' ? 'emergency' : 'scheduled',
        input.end_unknown ? 1 : 0,
        publishedAt(input.published_at),
      ],
    };
  }
  checkKeys(input, ['id', 'title', 'summary', 'body', 'place', 'starts_on', 'ends_on', 'link_url', 'published_at']);
  const startsOn = input.starts_on;
  const endsOn = input.ends_on || null;
  if (typeof startsOn !== 'string' || !DATE_RE.test(startsOn)
    || (endsOn != null && (typeof endsOn !== 'string' || !DATE_RE.test(endsOn) || endsOn < startsOn))) {
    throw new InputError('DATES_INVALID');
  }
  const link = input.link_url || null;
  if (link != null && (typeof link !== 'string' || !link.startsWith('https://') || link.length > 500 || /\s/.test(link))) {
    throw new InputError('LINK_URL_INVALID');
  }
  return {
    id: checkId(input.id),
    columns: ['title', 'summary', 'body', 'place', 'starts_on', 'ends_on', 'link_url', 'published_at'],
    values: [
      text(input.title, 200, { required: true }),
      text(input.summary, 300),
      text(input.body, 20000, { multiline: true }),
      text(input.place, 120),
      startsOn,
      endsOn,
      link,
      publishedAt(input.published_at),
    ],
  };
}

/** 길이가 같을 때만 비교하는 상수 시간 토큰 비교 (토큰은 32자 이상이어야 사용 가능) */
export function bearerOk(header, secret) {
  if (typeof secret !== 'string' || secret.length < 32) return false;
  const token = typeof header === 'string' && header.startsWith('Bearer ') ? header.slice(7) : '';
  if (token.length !== secret.length) return false;
  let diff = 0;
  for (let i = 0; i < token.length; i++) diff |= token.charCodeAt(i) ^ secret.charCodeAt(i);
  return diff === 0;
}

function json(data, status = 200, cache = 'no-store') {
  return Response.json(data, {
    status,
    headers: { 'Cache-Control': cache, 'X-Content-Type-Options': 'nosniff' },
  });
}
const error = (status, code) => json({ error: { code, message: code } }, status);

/** 요청 경로 → 콘텐츠 라우트 (해당 없으면 null) */
export function route(method, pathname) {
  let admin = false;
  let rest;
  if (pathname.startsWith('/api/content/')) { admin = true; rest = pathname.slice(13); }
  else if (pathname.startsWith('/api/')) rest = pathname.slice(5);
  else return null;
  if (!admin && rest === 'status') return (method === 'GET' || method === 'HEAD') ? { kind: 'status' } : { kind: 'method' };
  const [table, id, extra] = rest.split('/');
  if (!admin && !PUBLIC_TABLES.has(table)) return null;
  if (!TABLES.has(table) || extra !== undefined || (id !== undefined && id !== '' && !ID_RE.test(id))) {
    return admin ? { kind: 'notfound' } : null;
  }
  const hasId = !!id;
  const read = method === 'GET' || method === 'HEAD';
  if (!admin) {
    if (!read) return { kind: 'method' };
    return hasId ? { kind: 'get', table, id } : { kind: 'list', table };
  }
  if (read && !hasId) return { kind: 'adminList', table };
  if (method === 'POST' && !hasId) return { kind: 'create', table };
  if (method === 'PUT' && hasId) return { kind: 'update', table, id };
  if (method === 'DELETE' && hasId) return { kind: 'delete', table, id };
  return { kind: 'method' };
}

const columnsOf = table => ({ notices: NOTICE_COLUMNS, events: EVENT_COLUMNS, maintenance: MAINTENANCE_COLUMNS })[table];
const orderOf = table => ({ notices: 'pinned DESC, published_at DESC', events: 'starts_on DESC', maintenance: 'starts_at DESC' })[table];

const OFF = new Set(['', '0', 'off', 'false', 'no']);
/** 비상 스위치(Worker 시크릿 MAINTENANCE_MODE)가 켜져 있으면 지금부터의 긴급 점검 */
export function envMaintenance(env, now) {
  const mode = String(env?.MAINTENANCE_MODE ?? '').trim().toLowerCase();
  if (OFF.has(mode)) return null;
  const until = String(env.MAINTENANCE_UNTIL ?? '').trim();
  const known = TS_RE.test(until) && until > now;
  return {
    id: 'emergency-switch',
    title: String(env.MAINTENANCE_TITLE ?? '').trim() || '긴급 서버 점검',
    body: String(env.MAINTENANCE_MESSAGE ?? '').trim(),
    starts_at: now,
    ends_at: known ? until : '9999-12-31T00:00:00Z',
    kind: 'emergency',
    end_unknown: !known,
    published_at: now,
    updated_at: now,
  };
}

/** 지금 진행 중인 점검과, 72시간 안에 시작하는 예고된 점검 */
export function pickStatus(rows, now) {
  const nowMs = Date.parse(now);
  const live = rows.filter(r => r.published_at <= now && r.ends_at > now);
  const active = live.filter(r => r.starts_at <= now).sort((a, b) => a.starts_at.localeCompare(b.starts_at))[0] ?? null;
  const upcoming = live
    .filter(r => r.starts_at > now && Date.parse(r.starts_at) - nowMs <= NOTICE_AHEAD_MS)
    .sort((a, b) => a.starts_at.localeCompare(b.starts_at))[0] ?? null;
  return { now, maintenance: { active, upcoming } };
}

async function readJson(request) {
  try {
    const bytes = await readLimitedBody(request, MAX_BODY);
    return JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes));
  } catch (e) {
    throw new InputError(e instanceof RangeError ? 'PAYLOAD_TOO_LARGE' : 'INVALID_INPUT');
  }
}

// 점검 일정 행은 인스턴스 단위로 잠깐(10초) 기억한다. /api/status는 모든 탭이 1분마다 부르고,
// 점검 중 API 차단 검사는 모든 /api/* 요청마다 돌기 때문에 매번 D1을 읽으면 지연·CPU가 커진다.
// 판정(진행 중·예고)은 항상 현재 시각으로 다시 계산하므로 캐시가 시각을 늦추지 않는다.
const STATUS_TTL_MS = 10_000;
let rowsCache = new WeakMap();
export function resetMaintenanceCache() { rowsCache = new WeakMap(); }

async function maintenanceRows(db, now) {
  const hit = rowsCache.get(db);
  if (hit && Date.now() - hit.at < STATUS_TTL_MS) return hit.rows;
  // 캐시하는 동안 끝난 점검은 pickStatus가 현재 시각으로 걸러 낸다
  const pending = (async () => {
    try {
      const { results } = await db.prepare(
        `SELECT ${MAINTENANCE_COLUMNS} FROM maintenance WHERE deleted_at IS NULL AND ends_at > ?1 ORDER BY starts_at LIMIT 20`,
      ).bind(now).all();
      return (results ?? []).map(row => present('maintenance', row));
    } catch (e) {
      console.error('status error', e);
      return [];
    }
  })();
  // Concurrent API calls share the in-flight D1 read as well as its result.
  rowsCache.set(db, { at: Date.now(), rows: pending });
  return pending;
}

/** 비상 스위치 + D1 점검 일정. D1을 못 읽어도(마이그레이션 전 등) 스튜디오는 정상 동작하게 빈 상태로 답한다 */
export async function serviceStatus(env, now = nowUtc()) {
  const forced = envMaintenance(env, now);
  let status = { now, maintenance: { active: null, upcoming: null } };
  try {
    if (!env.CONTENT_DB) throw new Error('CONTENT_DB binding missing');
    status = pickStatus(await maintenanceRows(env.CONTENT_DB, now), now);
  } catch (e) {
    console.error('status error', e);
  }
  if (forced) status.maintenance.active = forced;
  return status;
}

/** 점검 중 API 차단용 — 비상 스위치가 켜져 있으면 D1을 읽지 않는다 */
async function activeMaintenance(env) {
  const now = nowUtc();
  return envMaintenance(env, now) ?? (await serviceStatus(env, now)).maintenance.active;
}

// ---- 공개 공지·이벤트 엣지 캐시 ----
// 로그인 없이 누구나 같은 응답을 받으므로 데이터센터 캐시(caches.default)에 30초 둔다.
// D1 조회와 JSON 직렬화를 건너뛰어 Worker CPU 시간을 줄인다. 관리자가 글을 바꾸면 이 데이터센터의 캐시는 바로 지운다.
const EDGE_TTL = 30;
const edgeCache = () => (typeof caches !== 'undefined' && caches.default) || null;
const cacheKey = (origin, path) => new Request(origin + path, { method: 'GET' });

async function cachedPublic(request, url, ctx, build) {
  const cache = edgeCache();
  if (!cache || request.method !== 'GET') return build();
  const key = cacheKey(url.origin, url.pathname);
  const hit = await cache.match(key);
  if (hit) return hit;
  const res = await build();
  if (res.status === 200) {
    const copy = res.clone();
    copy.headers.set('Cache-Control', `public, max-age=${EDGE_TTL}`);
    const put = cache.put(key, copy);
    if (ctx?.waitUntil) ctx.waitUntil(put); else await put;
  }
  return res;
}

async function purgePublic(url, table, id) {
  const cache = edgeCache();
  if (!cache || !PUBLIC_TABLES.has(table)) return;
  const paths = [`/api/${table}`];
  if (id) paths.push(`/api/${table}/${id}`);
  await Promise.all(paths.map(p => cache.delete(cacheKey(url.origin, p)).catch(() => false)));
}

async function readPublic(db, r, now) {
  const { table } = r;
  try {
    if (r.kind === 'list') {
      const { results } = await db.prepare(
        `SELECT ${columnsOf(table)} FROM ${table}
         WHERE deleted_at IS NULL AND published_at <= ?1 ORDER BY ${orderOf(table)} LIMIT 200`,
      ).bind(now).all();
      const today = todayKst();
      return json({ items: (results ?? []).map(row => present(table, row, today)) }, 200, PUBLIC_CACHE);
    }
    const row = await db.prepare(
      `SELECT ${columnsOf(table)} FROM ${table} WHERE id = ?1 AND deleted_at IS NULL AND published_at <= ?2`,
    ).bind(r.id, now).first();
    return row ? json(present(table, row), 200, PUBLIC_CACHE) : error(404, 'NOT_FOUND');
  } catch (e) {
    console.error('content error', e);
    return error(500, 'CONTENT_ERROR');
  }
}

/** D1 writes are authorized by the backend's current ADMIN session and CSRF.
 * The legacy operational token remains available for emergency tooling. */
export async function authorizeContent(request, env) {
  if (request.headers.has('Authorization')) {
    if (!env.CONTENT_ADMIN_TOKEN) return error(503, 'CONTENT_ADMIN_DISABLED');
    return bearerOk(request.headers.get('Authorization'), env.CONTENT_ADMIN_TOKEN) ? null : error(401, 'UNAUTHENTICATED');
  }
  const url = new URL(request.url);
  const read = request.method === 'GET' || request.method === 'HEAD';
  if (!read && (request.headers.get('origin') !== url.origin
    || (request.headers.get('sec-fetch-site') && request.headers.get('sec-fetch-site') !== 'same-origin'))) return error(403, 'FORBIDDEN');
  if (!request.headers.get('cookie')) return error(401, 'UNAUTHENTICATED');
  if (!env.EDGE_SERVICE_SECRET) return error(503, 'BACKEND_UNAVAILABLE');
  const headers = new Headers({ Accept: 'application/json', 'x-audeniq-service': env.EDGE_SERVICE_SECRET,
    origin: env.BACKEND_APP_ORIGIN || url.origin, 'sec-fetch-site': 'same-origin' });
  for (const key of ['cookie', 'x-csrf-token']) {
    const value = request.headers.get(key);
    if (value) headers.set(key, value);
  }
  try {
    const backend = backendOrigin(env);
    let res = await fetch(backend + '/api/staff/content-access', {
      method: read ? 'GET' : 'POST', headers, redirect: 'manual',
    });
    if (res.status === 200) return null;
    // During the API rollout, the existing staff endpoint still checks the
    // active role. Mutations also compare the supplied CSRF against the
    // backend's stable, session-bound token; this never authorizes on role alone.
    if (res.status === 404) {
      res = await fetch(backend + '/api/staff/me', { headers, redirect: 'manual' });
      if (res.status === 200) {
        const staff = await res.json();
        if (staff?.role !== 'ADMIN') return error(403, 'FORBIDDEN');
        if (read) return null;
        if (!request.headers.get('x-csrf-token')) return error(403, 'FORBIDDEN');
        res = await fetch(backend + '/api/auth/csrf', { method: 'POST', headers, redirect: 'manual' });
        if (res.status === 200) {
          const csrf = await res.json();
          return typeof csrf?.csrf_token === 'string'
            && bearerOk(`Bearer ${request.headers.get('x-csrf-token')}`, csrf.csrf_token)
            ? null : error(403, 'FORBIDDEN');
        }
      }
    }
    const data = await res.json().catch(() => null);
    return error(res.status >= 400 && res.status < 600 ? res.status : 502, data?.error?.code || 'FORBIDDEN');
  } catch { return error(502, 'BACKEND_UNAVAILABLE'); }
}

export async function handleContent(request, env, r, ctx) {
  if (r.kind === 'notfound') return error(404, 'NOT_FOUND');
  if (r.kind === 'method') return error(405, 'METHOD_NOT_ALLOWED');
  const db = env.CONTENT_DB;
  const { table } = r;
  const now = nowUtc();

  if (r.kind === 'status') return json(await serviceStatus(env, now), 200, 'no-store');
  if (!db) return error(503, 'CONTENT_UNAVAILABLE');

  if (r.kind === 'list' || r.kind === 'get') {
    return cachedPublic(request, new URL(request.url), ctx, () => readPublic(db, r, now));
  }
  // ---- 관리 ----
  const denied = await authorizeContent(request, env);
  if (denied) return denied;
  const res = await handleAdmin(request, db, r, now);
  // 쓰기가 끝난 뒤 — 점검 일정은 이 인스턴스의 API 차단 캐시를, 공지·이벤트는 이 데이터센터의 공개 캐시를 비운다
  if (r.kind !== 'adminList' && res.status < 500) {
    if (table === 'maintenance') resetMaintenanceCache();
    else await purgePublic(new URL(request.url), table, r.id);
  }
  return res;
}

async function handleAdmin(request, db, r, now) {
  const { table } = r;
  try {
    if (r.kind === 'adminList') {
      const { results } = await db.prepare(
        `SELECT ${columnsOf(table)}, created_at, deleted_at FROM ${table} ORDER BY ${orderOf(table)} LIMIT 500`,
      ).all();
      const today = todayKst();
      return json({ items: (results ?? []).map(row => present(table, row, today)), now });
    }
    if (r.kind === 'delete') {
      const res = await db.prepare(
        `UPDATE ${table} SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2 AND deleted_at IS NULL`,
      ).bind(now, r.id).run();
      return res.meta?.changes === 1 ? json({ id: r.id, deleted: true }) : error(404, 'NOT_FOUND');
    }

    const input = await readJson(request);
    const row = validate(table, input);
    if (r.kind === 'create') {
      const id = row.id ?? `${({ notices: 'n', events: 'e', maintenance: 'm' })[table]}-${now.slice(0, 10).replaceAll('-', '')}-${crypto.randomUUID().slice(0, 8)}`;
      const cols = ['id', ...row.columns];
      try {
        await db.prepare(
          `INSERT INTO ${table} (${cols.join(', ')}) VALUES (${cols.map((_, i) => `?${i + 1}`).join(', ')})`,
        ).bind(id, ...row.values).run();
      } catch (e) {
        if (/UNIQUE|PRIMARY KEY/i.test(String(e?.message ?? e))) return error(409, 'ID_TAKEN');
        throw e;
      }
      const created = await db.prepare(`SELECT ${columnsOf(table)} FROM ${table} WHERE id = ?1`).bind(id).first();
      return json(present(table, created), 201);
    }
    // update: 삭제된 글도 수정하면 다시 보이게 한다 (복구)
    if (row.id && row.id !== r.id) throw new InputError('ID_INVALID');
    const sets = row.columns.map((c, i) => `${c} = ?${i + 1}`).join(', ');
    const res = await db.prepare(
      `UPDATE ${table} SET ${sets}, updated_at = ?${row.columns.length + 1}, deleted_at = NULL WHERE id = ?${row.columns.length + 2}`,
    ).bind(...row.values, now, r.id).run();
    if (res.meta?.changes !== 1) return error(404, 'NOT_FOUND');
    const updated = await db.prepare(`SELECT ${columnsOf(table)} FROM ${table} WHERE id = ?1`).bind(r.id).first();
    return json(present(table, updated));
  } catch (e) {
    if (e instanceof InputError) return error(e.code === 'PAYLOAD_TOO_LARGE' ? 413 : 400, e.code);
    console.error('content error', e);
    return error(500, 'CONTENT_ERROR');
  }
}

// ---- 백엔드 프록시 ----
// 브라우저 헤더는 필요한 것만 골라 보낸다(crates/edge와 같은 허용 목록). 브라우저가 보낸
// x-audeniq-service·x-audeniq-client-ip 같은 서비스 헤더는 절대 전달하지 않는다.
// user-agent: 권리 서류 서명 기록(증거)에 남긴다
const FORWARD_HEADERS = ['cookie', 'origin', 'content-type', 'accept', 'x-csrf-token', 'sec-fetch-site', 'x-request-id', 'user-agent'];
const PARTNER_HOOK = /^\/api\/partner-hooks\//;
const PARTNER_DROP = new Set(['x-forwarded-for', 'x-real-ip', 'host', 'connection', 'accept-encoding']);

export function backendHeaders(request, env, pathname) {
  const src = request.headers;
  const headers = new Headers();
  for (const name of FORWARD_HEADERS) {
    const v = src.get(name);
    if (v) headers.set(name, v);
  }
  headers.set('accept-encoding', passThroughEncoding(clientEncoding(request)));
  if (PARTNER_HOOK.test(pathname)) {
    // 파트너 설정이 정한 서명·시각 헤더(이름 자유)는 전달하되 우리 이름공간·전달 IP 헤더는 제외
    for (const [name, value] of src) {
      if (!name.startsWith('x-audeniq-') && !PARTNER_DROP.has(name)) headers.set(name, value);
    }
  }
  // 로그인 시도 제한은 사용자 IP 기준 — Cloudflare가 매 요청 덮어쓰는 CF-Connecting-IP만 믿는다
  const ip = src.get('cf-connecting-ip');
  if (ip) headers.set('x-audeniq-client-ip', ip);
  headers.set('x-audeniq-service', env.EDGE_SERVICE_SECRET);
  return headers;
}

const MAINTENANCE_HEADERS = { 'Cache-Control': 'no-store', 'Retry-After': '60', 'X-Content-Type-Options': 'nosniff' };

async function proxy(request, env, url) {
  // 점검 중에는 백엔드로 보내지 않는다 (DB 작업 중 쓰기 방지, 스튜디오는 이 응답을 받자마자 점검 화면)
  // 콘텐츠 관리에 필요한 인증만 유지한다. 발매·심사 등 업무 데이터 변경은 점검 중 차단한다.
  const sessionAccess = (request.method === 'GET' && ['/api/me', '/api/staff/me'].includes(url.pathname))
    || (request.method === 'POST' && ['/api/auth/login', '/api/auth/logout', '/api/auth/csrf'].includes(url.pathname));
  const maint = url.pathname === '/ready' || sessionAccess ? null : await activeMaintenance(env);
  if (maint) {
    return Response.json({ error: { code: 'MAINTENANCE', message: maint.title }, maintenance: maint }, { status: 503, headers: MAINTENANCE_HEADERS });
  }
  if (!env.EDGE_SERVICE_SECRET) return error(503, 'BACKEND_UNAVAILABLE');
  const read = request.method === 'GET' || request.method === 'HEAD';
  let res;
  try {
    res = await fetch(new Request(backendOrigin(env) + url.pathname + url.search, {
      method: request.method,
      headers: backendHeaders(request, env, url.pathname),
      // 본문은 읽지 않고 스트림으로 넘긴다 (Worker 메모리·CPU 절약)
      body: read ? null : request.body,
      redirect: 'manual',
      duplex: 'half',
    }));
  } catch {
    return error(502, 'BACKEND_UNAVAILABLE');
  }
  // 본문을 건드리지 않고 헤더만 바꿔 돌려준다 — 압축된 응답이 다시 풀리거나 재압축되지 않는다
  const out = new Response(res.body, { status: res.status, statusText: res.statusText, headers: res.headers,
    ...(res.headers.has('content-encoding') ? { encodeBody: 'manual' } : {}) });
  out.headers.set('Access-Control-Allow-Origin', 'https://studio.audeniq.com');
  out.headers.set('Access-Control-Allow-Credentials', 'true');
  if (!out.headers.has('Cache-Control')) out.headers.set('Cache-Control', 'no-store');
  return out;
}

const studioWorker = {
  async fetch(request, env, ctx) {
    const url = new URL(request.url);
    const r = route(request.method, url.pathname);
    if (r) return handleContent(request, env, r, ctx);
    // /api/* 중 D1 콘텐츠가 아니면 백엔드로 프록시 (named tunnel audeniq-backend → compose api:8080)
    if (url.pathname.startsWith('/api/') || url.pathname === '/ready') return proxy(request, env, url);
    // 그 외는 정적 에셋 (없는 화면 경로는 index.html로 SPA 폴백)
    const assetRes = await serveAssets(request, env);
    if (assetRes.status === 404) {
      const isAsset = /\.[a-z0-9]+$/i.test(url.pathname);
      if (!isAsset) {
        const indexRes = await serveAssets(new Request(new URL('/', request.url), request), env);
        if (indexRes.ok) {
          // 에셋 응답의 보안 헤더(_headers의 CSP 등)는 그대로 두고 캐시만 끈다
          const headers = new Headers(indexRes.headers);
          headers.set('Cache-Control', 'no-cache');
          return new Response(indexRes.body, { status: 200, headers });
        }
      }
    }
    return assetRes;
  },
};

export default {
  async fetch(request, env, ctx) {
    const refused = requireHttps(request);
    if (refused) return refused;
    return secureResponse(await studioWorker.fetch(request, env, ctx));
  },
};
