import { describe, expect, it } from 'vitest';
import { countLabel, latestSummary, listeningSummary, prevMonth, type ReportRow } from './reports';

describe('prevMonth', () => {
  it('returns the calendar month before, across years', () => {
    expect(prevMonth('2026-09')).toBe('2026-08');
    expect(prevMonth('2026-01')).toBe('2025-12');
    expect(prevMonth('')).toBe('');
  });
});

describe('latestSummary', () => {
  const row = (period: string, revenue: number): ReportRow => ({ period, platform: 'Spotify', release: 'A', track: 'A', plays: 1, revenue });
  it('does not compare with a month two back when last month has no rows', () => {
    expect(latestSummary([row('2026-09', 200), row('2026-07', 100)]).change).toBeNull();
    expect(latestSummary([row('2026-09', 200), row('2026-08', 100)]).change).toBe(100);
  });
});

describe('listeningSummary', () => {
  const row = (period: string, plays: number, track = 'A', platform = 'Spotify', revenue = 1): ReportRow => ({ period, platform, release: 'R', track, plays, revenue });

  it('leads with plays; last month only when it is the calendar month before', () => {
    const s = listeningSummary([row('2026-09', 30000), row('2026-08', 26000)]);
    expect(s.plays).toBe(30000);
    expect(s.prevPlays).toBe(26000);
    expect(listeningSummary([row('2026-09', 30000), row('2026-07', 26000)]).prevPlays).toBeNull();
  });

  it('good news only: the first milestone crossed, else the best month, never a drop', () => {
    expect(listeningSummary([row('2026-09', 31000), row('2026-08', 26000)]).highlight).toBe('처음으로 한 달 3만 번을 넘었어요');
    expect(listeningSummary([row('2026-09', 28000), row('2026-08', 26000)]).highlight).toBe('가장 많이 들은 달이에요');
    expect(listeningSummary([row('2026-09', 20000), row('2026-08', 26000)]).highlight).toBeNull();
    // 첫 달은 비교할 달이 없다
    expect(listeningSummary([row('2026-09', 31000)]).highlight).toBeNull();
  });

  it('most played track and platform go by plays, not revenue', () => {
    const s = listeningSummary([row('2026-09', 10, 'Loud', 'Melon', 1), row('2026-09', 5, 'Rich', 'Spotify', 100)]);
    expect(s.topTrack).toBe('Loud');
    expect(s.topPlatform).toBe('Melon');
  });

  it('counts read in Korean units', () => {
    expect(countLabel(5000)).toBe('5,000');
    expect(countLabel(30000)).toBe('3만');
    expect(countLabel(1000000)).toBe('100만');
  });
});
