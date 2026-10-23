// 홈 화면 계산 — 다가오는 발매, 발매 단계, 마감까지 남은 날 (pages/Dashboard.tsx).
import type { Release } from '../api/types';
import { todayStr } from './date';

/** 접수 → 검토 → 배급 승인 → 발매. 보완 필요는 검토 단계에 머문 것으로 본다 */
export const RELEASE_STEPS = ['접수', '검토', '배급 승인', '발매'] as const;
const STEP: Record<string, number> = { ready: 0, review: 1, needs: 1, scheduled: 2, live: 3 };

export function releaseStep(status: string): number {
  return STEP[status] ?? -1;
}

/** 'YYYY-MM-DD' 두 날짜 사이의 날 수 (b − a) */
export function daysBetween(a: string, b: string): number {
  const d = (s: string) => Date.UTC(+s.slice(0, 4), +s.slice(5, 7) - 1, +s.slice(8, 10));
  return Math.round((d(b) - d(a)) / 86400000);
}

/** D-17 · 오늘 발매 · 발매 3일째 */
export function dDayLabel(date: string, today = todayStr()): string {
  const n = daysBetween(today, date);
  if (n > 0) return `D-${n}`;
  if (n === 0) return '오늘 발매';
  return `발매 ${1 - n}일째`;
}

/** 발매 뒤 이만큼은 홈에 계속 둔다 (플랫폼에 다 올라왔는지 보는 기간) */
const LIVE_DAYS = 14;

/**
 * 홈에 크게 보여 줄 발매 하나 — 접수 뒤 아직 나오지 않은 것 중 발매일이 가장 가까운 것,
 * 없으면 최근 2주 안에 나온 것. 작성 중·종료·거절·취소는 보지 않는다.
 */
export function upcomingRelease(releases: Release[], today = todayStr()): Release | undefined {
  const pending = releases
    .filter(r => ['ready', 'review', 'needs', 'scheduled'].includes(r.status))
    .sort((a, b) => (a.release_date ?? '9999').localeCompare(b.release_date ?? '9999'));
  if (pending.length) return pending[0];
  return releases
    .filter(r => r.status === 'live' && r.release_date && daysBetween(r.release_date, today) <= LIVE_DAYS)
    .sort((a, b) => (b.release_date ?? '').localeCompare(a.release_date ?? ''))[0];
}

/** 피칭을 낼 수 있는 마지막 날까지 남은 날 — 발매일 lead일 전이 마감 */
export function pitchDaysLeft(daysUntilRelease: number, leadDays: number): number {
  return daysUntilRelease - leadDays;
}
