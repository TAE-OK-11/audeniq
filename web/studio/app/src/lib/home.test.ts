import { describe, expect, it } from 'vitest';
import type { Release } from '../api/types';
import { dDayLabel, daysBetween, pitchDaysLeft, releaseStep, upcomingRelease } from './home';

const rel = (id: string, status: string, release_date: string | null): Release =>
  ({ id, title: id, status, release_date, created_at: '2026-10-01', track_count: 1 });

describe('홈 화면 계산', () => {
  it('날 수와 D-day', () => {
    expect(daysBetween('2026-10-24', '2026-11-09')).toBe(16);
    expect(daysBetween('2026-12-31', '2027-01-01')).toBe(1);
    expect(dDayLabel('2026-11-09', '2026-10-24')).toBe('D-16');
    expect(dDayLabel('2026-10-24', '2026-10-24')).toBe('오늘 발매');
    expect(dDayLabel('2026-10-22', '2026-10-24')).toBe('발매 3일째');
  });

  it('단계: 보완 필요는 검토, 작성 중은 단계 없음', () => {
    expect(releaseStep('ready')).toBe(0);
    expect(releaseStep('needs')).toBe(1);
    expect(releaseStep('scheduled')).toBe(2);
    expect(releaseStep('live')).toBe(3);
    expect(releaseStep('draft')).toBe(-1);
  });

  it('다가오는 발매: 접수 뒤 가장 가까운 발매일, 없으면 최근 2주 안에 나온 것', () => {
    const today = '2026-10-24';
    expect(upcomingRelease([
      rel('draft', 'draft', '2026-10-30'),
      rel('far', 'review', '2026-11-16'),
      rel('near', 'scheduled', '2026-11-09'),
      rel('nodate', 'ready', null),
    ], today)?.id).toBe('near');
    expect(upcomingRelease([rel('old', 'live', '2026-09-01'), rel('new', 'live', '2026-10-20')], today)?.id).toBe('new');
    expect(upcomingRelease([rel('old', 'live', '2026-09-01'), rel('d', 'draft', null)], today)).toBeUndefined();
  });

  it('피칭 마감은 발매일 lead일 전', () => {
    expect(pitchDaysLeft(16, 7)).toBe(9);
    expect(pitchDaysLeft(7, 7)).toBe(0);
  });
});
