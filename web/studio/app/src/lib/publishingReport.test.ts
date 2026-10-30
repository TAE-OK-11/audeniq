import { describe, expect, it } from 'vitest';
import type { Royalty } from '../api/publishing';
import { publishingReport } from './publishingReport';

const row = (period: string, net: number, status: Royalty['status'], workTitle = '이번 곡', source = 'KOMCA'): Royalty => ({
  id: `${period}-${net}`, workId: workTitle, workTitle, period, net, status, source,
  kind: 'digital', country: 'KR', currency: 'KRW', collected: net, share: 100, fee: 0, postedAt: null,
});

describe('publishing report presentation', () => {
  it('uses the latest confirmed month and keeps newer estimates out of the amount and trend', () => {
    const r = publishingReport([row('2026-06', 300, 'paid'), row('2026-07', 200, 'confirmed'), row('2026-08', 999, 'estimated')])!;
    expect(r.period).toBe('2026-07');
    expect(r.amount).toBe(200);
    expect(r.prev).toBe(300);
    expect(r.estimated).toBe(999);
    expect(r.trend).toEqual([{ period: '2026-06', value: 300 }, { period: '2026-07', value: 200 }]);
  });
  it('compares calendar months and ranks songs/sources in the displayed month', () => {
    const r = publishingReport([row('2025-12', 10000, 'paid', '예전 곡', 'JASRAC'), row('2026-02', 100, 'confirmed')])!;
    expect(r.prev).toBe(0);
    expect(r.topSong).toBe('이번 곡');
    expect(r.topSource).toBe('KOMCA');
    expect(r.trend.map(x => x.period)).toEqual(['2025-12', '2026-01', '2026-02']);
  });
  it('does not present an estimate-only account as having confirmed income', () => {
    const r = publishingReport([row('2026-08', 123, 'estimated')])!;
    expect(r.hasConfirmed).toBe(false);
    expect(r.amount).toBe(0);
    expect(r.estimated).toBe(123);
    expect(r.topSong).toBeUndefined();
    expect(publishingReport([])).toBeNull();
  });
});
