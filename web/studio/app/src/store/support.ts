// 알림 공유 스토어 — 라이브 db.notifications 대응
// Support(알림)와 Dashboard(읽지 않은 알림 수)가 같은 알림을 공유한다.
import { useSyncExternalStore } from 'react';

export interface Notice {
  id: string;
  kind: string;
  title: string;
  detail: string;
  time: string;
  read: boolean;
}

const INITIAL_NOTICES: Notice[] = [
  { id: 'n1', kind: '발매', title: '여름 EP 심사가 통과됐어요.', detail: '여름 EP가 모든 플랫폼 심사를 통과했어요. 발매일에 맞춰 순차적으로 송출될 예정이에요.', time: '2026-09-22', read: false },
  { id: 'n2', kind: '발매', title: '데모 트랙에 보완 요청이 있어요.', detail: '데모 트랙의 커버아트 해상도가 규격에 맞지 않아요. 3000×3000 이상으로 다시 등록해 주세요.', time: '2026-09-23', read: false },
  { id: 'n3', kind: '지급', title: '2026년 8월 정산이 확정됐어요.', detail: '2026년 8월 정산 ₩24,406이 확정됐어요. 지급 요청은 정산·지급 화면에서 할 수 있어요.', time: '2026-09-10', read: true },
];

let notices: Notice[] = INITIAL_NOTICES.map(n => ({ ...n }));
const listeners = new Set<() => void>();

function emit() {
  listeners.forEach(l => l());
}

function subscribe(fn: () => void) {
  listeners.add(fn);
  return () => { listeners.delete(fn); };
}

export function useNotices(): Notice[] {
  return useSyncExternalStore(subscribe, () => notices);
}

export function unreadCount(): number {
  return notices.filter(n => !n.read).length;
}

export function markNoticeRead(id: string) {
  notices = notices.map(n => (n.id === id ? { ...n, read: true } : n));
  emit();
}

export function markAllNoticesRead() {
  notices = notices.map(n => ({ ...n, read: true }));
  emit();
}
