import { describe, expect, it } from 'vitest';
import { entityLink, recurLabel, remaining } from './labels';

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
});
