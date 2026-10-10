import { describe, expect, it } from 'vitest';
import { josa } from './josa';

describe('josa', () => {
  it('받침에 따라 조사를 고른다', () => {
    expect(josa('커버아트', '을/를')).toBe('커버아트를');
    expect(josa('음원', '과/와')).toBe('음원과');
    expect(josa('크레딧', '을/를')).toBe('크레딧을');
    expect(josa('뒤', '으로/로')).toBe('뒤로');
    expect(josa('앞', '으로/로')).toBe('앞으로');
    expect(josa('서울', '으로/로')).toBe('서울로');
  });
  it('숫자는 읽는 소리로', () => {
    expect(josa(1, '은/는')).toBe('1은');
    expect(josa(2, '은/는')).toBe('2는');
    expect(josa('v2', '으로/로')).toBe('v2로');
    expect(josa('v3', '으로/로')).toBe('v3으로');
    expect(josa(7, '으로/로')).toBe('7로');
    expect(josa(10, '은/는')).toBe('10은');
  });
});
