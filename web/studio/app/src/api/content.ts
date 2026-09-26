// 공지·이벤트 — 엣지 Worker가 D1(CONTENT_DB)에서 바로 서빙한다.
// 읽기: /api/notices, /api/events (로그인 불필요)
// 관리: /api/content/{notices|events} (Authorization: Bearer <CONTENT_ADMIN_TOKEN>)
import { req } from './http';
import { ApiError, messageForCode } from './errors';
import { MOCK } from '../lib/mode';

// ---------------------------------------------------------------------------
// 공지·이벤트 (엣지 Worker + D1)
// ---------------------------------------------------------------------------
export interface ContentNotice { id: string; title: string; body: string; pinned: boolean; published_at: string }
export interface ContentEvent {
  id: string; title: string; summary: string; body: string; place: string;
  starts_on: string; ends_on: string | null; link_url: string | null; status: 'upcoming' | 'ongoing' | 'ended';
}
export const fetchNotices = () => req<{ items: ContentNotice[] }>('/api/notices', { quiet401: true }).then(r => items(r));
export const fetchEvents = () => req<{ items: ContentEvent[] }>('/api/events', { quiet401: true }).then(r => items(r));

function items<T>(r: { items?: T[] } | null): T[] {
  if (!Array.isArray(r?.items)) throw new ApiError('공지 서버에 연결하지 못했어요.', 0, 'CONTENT_UNAVAILABLE');
  return r.items;
}

/**
 * 공지·이벤트는 로그인·체험 모드와 관계없이 항상 D1(Worker)에서 읽는다.
 * 체험 모드에서 Worker가 아예 없을 때(로컬 vite, 정적 미리보기)만 예시 글을 보여 준다.
 */
export async function loadContent<T, U>(fetcher: () => Promise<T[]>, map: (row: T) => U, demo: U[]): Promise<U[]> {
  try {
    return (await fetcher()).map(map);
  } catch (e) {
    // Worker가 없어 다른 서버(로컬 프록시 등)가 답한 경우만. Worker의 오류(CONTENT_*)는 그대로 보여 준다.
    const unavailable = e instanceof ApiError && !e.code.startsWith('CONTENT_')
      && [0, 404, 405, 502, 503, 504].includes(e.status);
    if (MOCK && unavailable) return demo;
    throw e;
  }
}

// ---------------------------------------------------------------------------
// 관리 (콘텐츠 관리자 토큰)
// ---------------------------------------------------------------------------
export type ContentKind = 'notices' | 'events';

export interface AdminNotice extends ContentNotice { created_at: string; updated_at: string; deleted_at: string | null }
export interface AdminEvent extends ContentEvent { published_at: string; created_at: string; updated_at: string; deleted_at: string | null }
export type NoticeInput = { title: string; body: string; pinned: boolean; published_at: string };
export type EventInput = {
  title: string; summary: string; body: string; place: string;
  starts_on: string; ends_on: string | null; link_url: string | null; published_at: string;
};

const ADMIN_MESSAGES: Record<string, string> = {
  UNAUTHENTICATED: '관리자 토큰이 맞지 않아요.',
  CONTENT_ADMIN_DISABLED: 'Worker에 CONTENT_ADMIN_TOKEN이 설정되지 않았어요.',
  CONTENT_UNAVAILABLE: 'D1(CONTENT_DB)에 연결하지 못했어요.',
  TITLE_REQUIRED: '제목을 입력해 주세요.',
  TOO_LONG: '입력한 내용이 너무 길어요.',
  TEXT_INVALID_CHARACTERS: '쓸 수 없는 문자가 들어 있어요.',
  PUBLISHED_AT_INVALID: '게시 시각을 확인해 주세요.',
  DATES_INVALID: '이벤트 기간을 확인해 주세요. (종료일은 시작일 이후)',
  LINK_URL_INVALID: '링크는 https://로 시작해야 해요.',
  ID_TAKEN: '같은 ID의 글이 이미 있어요.',
  NOT_FOUND: '글을 찾을 수 없어요. 목록을 새로 불러와 주세요.',
  PAYLOAD_TOO_LARGE: '본문이 너무 길어요.',
};

async function admin<T>(token: string, method: 'GET' | 'POST' | 'PUT' | 'DELETE', path: string, body?: unknown): Promise<T> {
  let res: Response;
  try {
    res = await fetch(path, {
      method,
      headers: { Accept: 'application/json', Authorization: `Bearer ${token}`, ...(body ? { 'Content-Type': 'application/json' } : {}) },
      body: body ? JSON.stringify(body) : undefined,
      cache: 'no-store',
      credentials: 'omit',
    });
  } catch {
    throw new ApiError('네트워크 연결을 확인해 주세요.', 0, 'NETWORK');
  }
  const data = await res.json().catch(() => null) as { error?: { code?: string } } | null;
  if (!res.ok || !data) {
    const code = data?.error?.code ?? (res.status === 404 || res.status === 405 ? 'CONTENT_API_MISSING' : '');
    const msg = code === 'CONTENT_API_MISSING'
      ? '콘텐츠 관리 API를 찾을 수 없어요. Worker가 배포됐는지 확인해 주세요.'
      : ADMIN_MESSAGES[code] ?? messageForCode(code, res.status);
    throw new ApiError(msg, res.status, code);
  }
  return data as T;
}

export const contentAdmin = {
  list: <K extends ContentKind>(token: string, kind: K) =>
    admin<{ items: (K extends 'notices' ? AdminNotice : AdminEvent)[]; now: string }>(token, 'GET', `/api/content/${kind}`),
  create: (token: string, kind: ContentKind, input: NoticeInput | EventInput) =>
    admin<{ id: string }>(token, 'POST', `/api/content/${kind}`, input),
  update: (token: string, kind: ContentKind, id: string, input: NoticeInput | EventInput) =>
    admin<{ id: string }>(token, 'PUT', `/api/content/${kind}/${encodeURIComponent(id)}`, input),
  remove: (token: string, kind: ContentKind, id: string) =>
    admin<{ id: string }>(token, 'DELETE', `/api/content/${kind}/${encodeURIComponent(id)}`),
};
