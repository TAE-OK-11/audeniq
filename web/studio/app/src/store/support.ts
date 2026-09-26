// 알림 공유 스토어 — Notifications(목록), Dashboard/Layout(읽지 않은 수)가 공유
import { createStore } from '../lib/store';

export interface Notice {
  id: string;
  kind: string;
  title: string;
  detail: string;
  time: string;
  read: boolean;
  /** 관련 화면 경로 (예: /releases/r2) */
  link?: string;
}

const INITIAL_NOTICES: Notice[] = [
  { id: 'n1', kind: '발매', title: '여름 EP 검토가 시작됐어요.', detail: '여름 EP의 발매 신청이 접수돼 AUDENIQ 담당자가 검토하고 있어요. 결과는 알림으로 알려드릴게요.', time: '2026-09-22 09:10', read: false, link: '/releases/r2' },
  { id: 'n2', kind: '발매', title: '데모 트랙에 보완 요청이 있어요.', detail: '커버아트 해상도와 작곡가 정보, 2건의 보완이 필요해요. 발매 관리에서 ‘보완하기’를 누르면 고칠 곳으로 바로 이동해요.', time: '2026-09-23 11:20', read: false, link: '/releases/r3' },
  { id: 'n3', kind: '지급', title: '2026년 8월 정산이 확정됐어요.', detail: '2026년 8월 정산 ₩24,406이 확정됐어요. 지급 요청은 정산·지급 화면에서 할 수 있어요.', time: '2026-09-10 10:00', read: true, link: '/settlement' },
];

const store = createStore<Notice[]>(INITIAL_NOTICES, {
  persist: 'notifications',
  revive: (raw, fallback) => (Array.isArray(raw)
    ? (raw as Notice[]).filter(n => n && typeof n.id === 'string' && typeof n.title === 'string')
      .map(n => ({ ...n, kind: String(n.kind ?? ''), detail: String(n.detail ?? ''), time: String(n.time ?? ''), read: !!n.read }))
    : fallback),
});

export const useNotices = store.use;

export function useUnreadCount(): number {
  return store.use().filter(n => !n.read).length;
}

export function markNoticeRead(id: string) {
  store.set(list => (list.some(n => n.id === id && !n.read) ? list.map(n => (n.id === id ? { ...n, read: true } : n)) : list));
}

export function markAllNoticesRead() {
  store.set(list => list.map(n => (n.read ? n : { ...n, read: true })));
}

export function pushNotice(n: Omit<Notice, 'read'>) {
  store.set(list => [{ ...n, read: false }, ...list]);
}
