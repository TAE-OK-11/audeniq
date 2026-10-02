// 공지·이벤트 — 엣지 Worker가 D1(CONTENT_DB)에서 바로 서빙한다.
// 읽기: /api/notices, /api/events (로그인 불필요)
// 관리: /api/content/{notices|events} (관리자 세션 + CSRF)
import { req } from './http';
import { ApiError } from './errors';
import { MOCK } from '../lib/mode';

// ---------------------------------------------------------------------------
// 공지·이벤트 (엣지 Worker + D1)
// ---------------------------------------------------------------------------
export interface ContentNotice { id: string; title: string; body: string; pinned: boolean; published_at: string }
export interface ContentEvent {
  id: string; title: string; summary: string; body: string; place: string;
  starts_on: string; ends_on: string | null; link_url: string | null; status: 'upcoming' | 'ongoing' | 'ended';
}
export const fetchNotices = () => req<{ items: ContentNotice[] }>('/api/notices', { quiet401: true, quietServer: true }).then(r => items(r));
export const fetchEvents = () => req<{ items: ContentEvent[] }>('/api/events', { quiet401: true, quietServer: true }).then(r => items(r));

/** 서버 점검 일정 (D1) — 진행 중인 점검과 72시간 안의 예고 */
export interface MaintenanceWindow {
  id: string; title: string; body: string; starts_at: string; ends_at: string; updated_at: string;
  /** 관리 화면의 '긴급 점검 시작'·비상 스위치로 시작된 점검 */
  kind?: 'scheduled' | 'emergency';
  /** 끝나는 시각을 모름 (ends_at은 임시 값) */
  end_unknown?: boolean;
}
export interface ServiceStatus { now: string; maintenance: { active: MaintenanceWindow | null; upcoming: MaintenanceWindow | null } }
export async function fetchStatus(): Promise<ServiceStatus | null> {
  try {
    const r = await req<ServiceStatus>('/api/status', { quiet401: true, quietServer: true, timeoutMs: 8000 });
    return r && typeof r === 'object' && r.maintenance ? r : null;
  } catch {
    return null; // 상태를 못 읽어도 스튜디오는 그대로 쓴다
  }
}

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
// 관리 (ADMIN 세션 + CSRF)
// ---------------------------------------------------------------------------
export type ContentKind = 'notices' | 'events' | 'maintenance';

export interface AdminNotice extends ContentNotice { created_at: string; updated_at: string; deleted_at: string | null }
export interface AdminEvent extends ContentEvent { published_at: string; created_at: string; updated_at: string; deleted_at: string | null }
export interface AdminMaintenance extends MaintenanceWindow { published_at: string; created_at: string; deleted_at: string | null }
export type MaintenanceInput = {
  title: string; body: string; starts_at: string; ends_at: string; published_at: string;
  kind?: 'scheduled' | 'emergency'; end_unknown?: boolean;
};
export type NoticeInput = { title: string; body: string; pinned: boolean; published_at: string };
export type EventInput = {
  title: string; summary: string; body: string; place: string;
  starts_on: string; ends_on: string | null; link_url: string | null; published_at: string;
};

export const contentAdmin = {
  list: <K extends ContentKind>(kind: K) =>
    req<{ items: (K extends 'notices' ? AdminNotice : K extends 'events' ? AdminEvent : AdminMaintenance)[]; now: string }>(`/api/content/${kind}`, { quietServer: true }),
  create: (kind: ContentKind, input: NoticeInput | EventInput | MaintenanceInput) =>
    req<{ id: string }>(`/api/content/${kind}`, { method: 'POST', body: input, quietServer: true }),
  update: (kind: ContentKind, id: string, input: NoticeInput | EventInput | MaintenanceInput) =>
    req<{ id: string }>(`/api/content/${kind}/${encodeURIComponent(id)}`, { method: 'PUT', body: input, quietServer: true }),
  remove: (kind: ContentKind, id: string) =>
    req<{ id: string }>(`/api/content/${kind}/${encodeURIComponent(id)}`, { method: 'DELETE', quietServer: true }),
};
