/**
 * AUDENIQ STUDIO Worker
 * - 정적 에셋(React 빌드)은 Workers Static Assets가 서빙
 * - 공지·이벤트는 D1(CONTENT_DB)에서 서빙
 *
 * 공개 (로그인 없이 읽기)
 *   GET  /api/notices, /api/notices/:id
 *   GET  /api/events,  /api/events/:id
 * 관리 (Authorization: Bearer <CONTENT_ADMIN_TOKEN>, wrangler secret)
 *   GET    /api/content/notices|events        예약·숨김 글까지 전체 목록
 *   POST   /api/content/notices|events        새 글
 *   PUT    /api/content/notices|events/:id    수정
 *   DELETE /api/content/notices|events/:id    삭제 (deleted_at 기록, 복구 가능)
 *
 * 경로·입력 규칙은 crates/edge/src/content.rs(Rust 엣지)와 같다.
 */

const TABLES = new Set(['notices', 'events']);
const NOTICE_COLUMNS = 'id, title, body, pinned, published_at, updated_at';
const EVENT_COLUMNS = 'id, title, summary, body, place, starts_on, ends_on, link_url, published_at, updated_at';
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
  const [table, id, extra] = rest.split('/');
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

const columnsOf = table => (table === 'notices' ? NOTICE_COLUMNS : EVENT_COLUMNS);
const orderOf = table => (table === 'notices' ? 'pinned DESC, published_at DESC' : 'starts_on DESC');

async function readJson(request) {
  const raw = await request.text();
  if (raw.length > MAX_BODY) throw new InputError('PAYLOAD_TOO_LARGE');
  try { return JSON.parse(raw); } catch { throw new InputError('INVALID_INPUT'); }
}

export async function handleContent(request, env, r) {
  if (r.kind === 'notfound') return error(404, 'NOT_FOUND');
  if (r.kind === 'method') return error(405, 'METHOD_NOT_ALLOWED');
  const db = env.CONTENT_DB;
  if (!db) return error(503, 'CONTENT_UNAVAILABLE');
  const { table } = r;
  const now = nowUtc();

  try {
    if (r.kind === 'list') {
      const { results } = await db.prepare(
        `SELECT ${columnsOf(table)} FROM ${table}
         WHERE deleted_at IS NULL AND published_at <= ?1 ORDER BY ${orderOf(table)} LIMIT 200`,
      ).bind(now).all();
      const today = todayKst();
      return json({ items: (results ?? []).map(row => present(table, row, today)) }, 200, PUBLIC_CACHE);
    }
    if (r.kind === 'get') {
      const row = await db.prepare(
        `SELECT ${columnsOf(table)} FROM ${table} WHERE id = ?1 AND deleted_at IS NULL AND published_at <= ?2`,
      ).bind(r.id, now).first();
      return row ? json(present(table, row), 200, PUBLIC_CACHE) : error(404, 'NOT_FOUND');
    }

    // ---- 관리 ----
    if (!env.CONTENT_ADMIN_TOKEN) return error(503, 'CONTENT_ADMIN_DISABLED');
    if (!bearerOk(request.headers.get('Authorization'), env.CONTENT_ADMIN_TOKEN)) return error(401, 'UNAUTHENTICATED');

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
      const id = row.id ?? `${table === 'notices' ? 'n' : 'e'}-${now.slice(0, 10).replaceAll('-', '')}-${crypto.randomUUID().slice(0, 8)}`;
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

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const r = route(request.method, url.pathname);
    if (r) return handleContent(request, env, r);
    // /api/* 중 D1 콘텐츠가 아니면 테스트 백엔드로 프록시
    if (url.pathname.startsWith('/api/')) {
      const backend = 'https://binding-textile-tale-ccd.trycloudflare.com';
      const backendUrl = backend + url.pathname + url.search;
      const proxyReq = new Request(backendUrl, request);
      try {
        const res = await fetch(proxyReq);
        // CORS 헤더 추가 (프론트에서 직접 호출 대비)
        const headers = new Headers(res.headers);
        headers.set('Access-Control-Allow-Origin', 'https://studio.audeniq.com');
        headers.set('Access-Control-Allow-Credentials', 'true');
        return new Response(res.body, { status: res.status, headers });
      } catch (e) {
        return error(502, 'BACKEND_UNAVAILABLE');
      }
    }
    // 그 외는 정적 에셋 (없는 화면 경로는 index.html로 SPA 폴백)
    const assetRes = await env.ASSETS.fetch(request);
    if (assetRes.status === 404) {
      const isAsset = /\.[a-z0-9]+$/i.test(url.pathname);
      if (!isAsset) {
        const indexRes = await env.ASSETS.fetch(new Request(new URL('/', request.url), request));
        if (indexRes.ok) {
          return new Response(indexRes.body, {
            status: 200,
            headers: { 'Content-Type': 'text/html;charset=utf-8', 'Cache-Control': 'no-cache' },
          });
        }
      }
    }
    return assetRes;
  },
};
