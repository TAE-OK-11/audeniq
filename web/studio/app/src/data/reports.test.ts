import { describe, expect, it } from 'vitest';
import { latestSummary, prevMonth, type ReportRow } from './reports';

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
