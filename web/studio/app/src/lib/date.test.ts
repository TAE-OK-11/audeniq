import { describe, expect, it } from 'vitest';
import { formatKoreanDate, monthKey, parseStamp, relativeTime, stampNow, todayStr } from './date';

describe('date utils', () => {
  it('Safari에서 실패하던 공백 구분 스탬프를 파싱한다', () => {
    const d = parseStamp('2026-09-20 14:32');
    expect(d?.getFullYear()).toBe(2026);
    expect(d?.getMonth()).toBe(8);
    expect(d?.getHours()).toBe(14);
    expect(d?.getMinutes()).toBe(32);
  });
  it('잘못된 값은 null', () => {
    expect(parseStamp('')).toBeNull();
    expect(parseStamp('not a date')).toBeNull();
    expect(parseStamp(undefined)).toBeNull();
  });
  it('형식 함수', () => {
    const d = new Date(2026, 0, 5, 9, 7);
    expect(todayStr(d)).toBe('2026-01-05');
    expect(stampNow(d)).toBe('2026-01-05 09:07');
    expect(monthKey(-1, d)).toBe('2025-12');
    expect(formatKoreanDate('2026-10-01')).toBe('2026년 10월 1일');
  });
  it('상대 시간', () => {
    const now = new Date(2026, 8, 26, 12, 0);
    expect(relativeTime('2026-09-26 11:58', now)).toBe('2분 전');
    expect(relativeTime('2026-09-24 12:00', now)).toBe('2일 전');
    expect(relativeTime('2026-08-01', now)).toBe('2026-08-01');
  });
});
