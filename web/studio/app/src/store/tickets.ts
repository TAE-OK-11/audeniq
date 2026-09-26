// 문의 기록 스토어
import { createStore } from '../lib/store';

export interface Ticket {
  id: string; category: string; releaseId: string; releaseTitle: string;
  subject: string; body: string; created: string; status: string;
}

const INITIAL_TICKETS: Ticket[] = [
  { id: 'q1', category: '발매·심사', releaseId: 'r3', releaseTitle: '데모 트랙', subject: '데모 트랙 보완 요청 관련 문의', body: '보완 요청 항목 중 커버아트 규격이 궁금해요.', created: '2026-09-23', status: '답변 완료' },
  { id: 'q2', category: '정산·지급', releaseId: '', releaseTitle: '', subject: '정산 금액 확인 요청', body: '2026년 8월 정산 금액의 상세 내역을 확인하고 싶어요.', created: '2026-09-21', status: '답변 대기' },
];

export const ticketsStore = createStore<Ticket[]>(INITIAL_TICKETS, {
  persist: 'tickets',
  revive: (raw, fb) => (Array.isArray(raw) ? raw as Ticket[] : fb),
});
