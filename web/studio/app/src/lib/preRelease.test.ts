import { describe, expect, it } from 'vitest';
import { preReleaseAction, preReleaseState, preorderChoices } from './preRelease';

describe('플랫폼 사전 공개', () => {
  it('단계', () => {
    expect(preReleaseState('2026-11-01', '2026-11-15', '2026-10-24')).toEqual({ phase: 'scheduled', daysToOpen: 8, daysToRelease: 22 });
    expect(preReleaseState('2026-10-20', '2026-11-15', '2026-10-24').phase).toBe('open');
    expect(preReleaseState(null, '2026-11-15', '2026-10-24').phase).toBe('none');
    expect(preReleaseState('2026-10-01', '2026-10-10', '2026-10-24').phase).toBe('released');
    expect(preReleaseState('2026-10-01', null).phase).toBe('none');
  });
  it('빠른 선택은 지나지 않은 날만', () => {
    expect(preorderChoices('2026-11-15', '2026-10-24')).toEqual([
      { weeks: 1, date: '2026-11-08' }, { weeks: 2, date: '2026-11-01' }, { weeks: 4, date: '2026-10-18' },
    ].filter(c => c.date >= '2026-10-24'));
    expect(preorderChoices('2026-10-27', '2026-10-24')).toEqual([]);
  });
  it('버튼 글자', () => {
    expect(preReleaseAction('spotify')).toBe('미리 저장');
    expect(preReleaseAction('apple')).toBe('미리 추가');
    expect(preReleaseAction('melon')).toBeNull();
  });
});
