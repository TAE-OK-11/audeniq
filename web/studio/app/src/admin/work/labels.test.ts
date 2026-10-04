import { describe, expect, it } from 'vitest';
import { entityLink, minutesLabel, recurLabel, remaining, slaTypeLabel } from './labels';

describe('업무 표기', () => {
  it('남은 시간', () => {
    const now = Date.parse('2026-10-05T01:30:00Z');
    expect(remaining('2026-10-05T02:00:00Z', now)).toBe('30분 남음');
    expect(remaining('2026-10-04T22:30:00Z', now)).toBe('3시간 지남');
    expect(remaining('2026-10-08T01:30:00Z', now)).toBe('3일 남음');
    expect(remaining(null, now)).toBe('');
  });
  it('반복 주기', () => {
    expect(recurLabel({ freq: 'WEEKLY', interval: 1 })).toBe('매주');
    expect(recurLabel({ freq: 'MONTHLY', interval: 3 })).toBe('3개월마다');
  });
  it('Entity가 열리는 관리자 주소', () => {
    expect(entityLink('release', 'r1')).toBe('/admin/reviews/r1');
    expect(entityLink('task', 't1')).toBe('/admin/tasks/t1');
    expect(entityLink('artist', 'a1')).toBeNull();
  });
  it('SLA 길이와 대상 이름', () => {
    expect(minutesLabel(120)).toBe('2시간');
    expect(minutesLabel(90)).toBe('1시간 30분');
    expect(minutesLabel(540, 540)).toBe('1영업일');
    expect(minutesLabel(1620 + 60, 540)).toBe('3영업일 1시간');
    expect(slaTypeLabel('SUPPORT_INQUIRY')).toBe('고객 문의');
    expect(slaTypeLabel('URGENT_DELIVERY')).toBe('긴급 송출');
  });
});
