import { describe, expect, it } from 'vitest';
import { classifyServerIssue } from './systemEvents';
import { kstWhen, windowLabel } from '../components/SystemStatus';

describe('classifyServerIssue', () => {
  it('게이트웨이·DB 장애는 down, MAINTENANCE는 점검, 나머지는 무시', () => {
    expect(classifyServerIssue(502, 'BACKEND_UNAVAILABLE')).toEqual({ kind: 'down', status: 502, code: 'BACKEND_UNAVAILABLE' });
    expect(classifyServerIssue(504, '')).toEqual({ kind: 'down', status: 504, code: 'HTTP_504' });
    expect(classifyServerIssue(503, 'MAINTENANCE')?.kind).toBe('maintenance');
    expect(classifyServerIssue(500, 'INTERNAL_ERROR')).toBeNull();
    expect(classifyServerIssue(404, 'NOT_FOUND')).toBeNull();
    expect(classifyServerIssue(409, 'CONFLICT')).toBeNull();
  });
});

describe('점검 시각 표기 (한국 시간)', () => {
  it('같은 날이면 끝 시각만, 날짜가 다르면 둘 다', () => {
    expect(kstWhen('2026-09-29T17:00:00Z')).toBe('9월 30일(수) 02:00');
    expect(windowLabel({ starts_at: '2026-09-29T17:00:00Z', ends_at: '2026-09-29T19:30:00Z' })).toBe('9월 30일(수) 02:00 ~ 04:30');
    expect(windowLabel({ starts_at: '2026-09-29T14:00:00Z', ends_at: '2026-09-29T16:00:00Z' })).toBe('9월 29일(화) 23:00 ~ 9월 30일(수) 01:00');
  });
});
