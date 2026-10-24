import { describe, expect, it } from 'vitest';
import { countdownLabel, countdownParts, releaseMoment } from './countdown';

describe('발매 카운트다운', () => {
  it('한국 시간 기준 발매 순간', () => {
    expect(new Date(releaseMoment('2026-11-15')).toISOString()).toBe('2026-11-14T15:00:00.000Z');
    expect(new Date(releaseMoment('2026-11-15', '18:00')).toISOString()).toBe('2026-11-15T09:00:00.000Z');
  });
  it('남은 시간 표기, 지나면 없음', () => {
    const t = releaseMoment('2026-11-15');
    expect(countdownLabel(t, t - (3 * 86400 + 12 * 3600 + 4 * 60 + 31) * 1000)).toBe('D-3 · 12:04:31');
    expect(countdownLabel(t, t - 59 * 1000)).toBe('00:00:59');
    expect(countdownLabel(t, t)).toBeNull();
  });
  it('칸별 남은 시간', () => {
    const t = releaseMoment('2026-11-15');
    expect(countdownParts(t, t - (3 * 86400 + 12 * 3600 + 4 * 60 + 31) * 1000)).toEqual({ d: 3, h: '12', m: '04', s: '31' });
    expect(countdownParts(t, t)).toBeNull();
  });
});
