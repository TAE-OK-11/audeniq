// 플랫폼별 배급 진행 단계 (서버 delivery_staging 기준) — 발매 상세 '배급·계약' 탭과 홈 '플랫폼 반영' 카드가 같이 쓴다
import type { Tone } from '../ui/tone';
import type { DeliveryItem } from '../api/types';

export const DELIVERY_STAGE: Record<string, string> = {
  NEEDS_CORRECTION: '보완 필요',
  IN_REVIEW: '배급 검토 중',
  PREPARING: '플랫폼 연결 준비 중',
  SCHEDULED: '전송 대기',
  ON_HOLD: '보류',
  SENDING: '전송 중',
  DELIVERED: '전송 완료',
  LIVE: '공개됨',
  TAKEN_DOWN: '내려감',
};

export const DELIVERY_TONE: Record<string, Tone> = {
  NEEDS_CORRECTION: 'amber', ON_HOLD: 'amber', IN_REVIEW: 'violet', PREPARING: 'blue', SCHEDULED: 'blue', SENDING: 'blue',
  DELIVERED: 'green', LIVE: 'green', TAKEN_DOWN: 'gray',
};

/** 많이 온 순서 — 홈 카드는 공개된 곳부터 보여 준다 */
const RANK: Record<string, number> = { LIVE: 0, DELIVERED: 1, SENDING: 2, SCHEDULED: 3, PREPARING: 4, IN_REVIEW: 5, ON_HOLD: 6, NEEDS_CORRECTION: 7, TAKEN_DOWN: 8 };

export interface DeliverySummary { total: number; live: number; sent: number; items: DeliveryItem[] }

/** 몇 곳 중 몇 곳이 공개·전송됐는지 — 내려간 곳은 전체에서 뺀다 */
export function deliverySummary(items: DeliveryItem[]): DeliverySummary {
  const active = items.filter(i => i.stage !== 'TAKEN_DOWN');
  return {
    total: active.length,
    live: active.filter(i => i.stage === 'LIVE').length,
    sent: active.filter(i => i.stage === 'LIVE' || i.stage === 'DELIVERED').length,
    items: [...active].sort((a, b) => (RANK[a.stage] ?? 9) - (RANK[b.stage] ?? 9)),
  };
}
