import { describe, expect, it } from 'vitest';
import { ago, dday, eventDay, eventWhen, isoToLocal, localToIso } from './artistCommunity';

describe('artist community helpers', () => {
  const now = new Date('2026-10-24T03:00:00Z'); // 한국 10월 24일 정오
  it('formats event time in Korea time, year only when not this year', () => {
    expect(eventWhen('2026-11-01T10:00:00Z', now)).toBe('11월 1일 (일) 오후 7:00');
    expect(eventWhen('2027-01-02T01:30:00Z', now)).toBe('2027년 1월 2일 (토) 오전 10:30');
    expect(eventDay('2026-10-31T16:00:00Z')).toEqual({ month: '11', day: '1', weekday: '일' });
  });
  it('counts days by the Korean calendar', () => {
    expect(dday('2026-10-24T16:00:00Z', now)).toEqual({ text: 'D-1', past: false }); // 25일 새벽 1시
    expect(dday('2026-10-24T10:00:00Z', now)).toEqual({ text: '오늘', past: false });
    expect(dday('2026-10-23T10:00:00Z', now)).toEqual({ text: '지난 이벤트', past: true });
  });
  it('round-trips datetime-local values as Korea time', () => {
    expect(localToIso('2026-11-01T19:00')).toBe('2026-11-01T10:00:00.000Z');
    expect(isoToLocal('2026-11-01T10:00:00Z')).toBe('2026-11-01T19:00');
    expect(localToIso('')).toBeNull();
  });
  it('says how long ago', () => {
    expect(ago('2026-10-24T02:59:30Z', now)).toBe('방금');
    expect(ago('2026-10-24T01:00:00Z', now)).toBe('2시간 전');
    expect(ago('2026-09-01T01:00:00Z', now)).toBe('2026.9.1');
  });
});
