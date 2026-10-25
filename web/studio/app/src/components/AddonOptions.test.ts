import { describe, expect, it } from 'vitest';
import { formatDuration } from './AddonOptions';

describe('formatDuration', () => {
  it('두 자리 뒤에 콜론을 넣는다', () => {
    expect(formatDuration('0')).toBe('0');
    expect(formatDuration('01')).toBe('01');
    expect(formatDuration('010')).toBe('01:0');
    expect(formatDuration('0101')).toBe('01:01');
  });
  it('숫자 말고는 버리고 네 자리까지만 받는다', () => {
    expect(formatDuration('03:45')).toBe('03:45');
    expect(formatDuration('1a2b34567')).toBe('12:34');
  });
  it('콜론을 지우면 두 자리로 돌아간다', () => {
    expect(formatDuration('01:')).toBe('01');
  });
});
