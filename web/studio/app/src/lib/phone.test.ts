import { describe, expect, it } from 'vitest';
import { formatPhone, isPhone } from './phone';

describe('formatPhone', () => {
  it('입력하는 대로 하이픈을 넣는다', () => {
    expect(formatPhone('010')).toBe('010');
    expect(formatPhone('0101234')).toBe('010-1234');
    expect(formatPhone('01012345678')).toBe('010-1234-5678');
    expect(formatPhone('0161234567')).toBe('016-123-4567');
  });
  it('국가번호(+82)가 붙어 오면 0으로 바꾼다', () => {
    expect(formatPhone('+82 10-1234-5678')).toBe('010-1234-5678');
    expect(formatPhone('+821012345678')).toBe('010-1234-5678');
    expect(formatPhone('82-10-1234-5678')).toBe('010-1234-5678');
    expect(isPhone(formatPhone('+82 10 1234 5678'))).toBe(true);
  });
  it('82로 시작하는 국내 번호 입력 중간 상태는 건드리지 않는다', () => {
    expect(formatPhone('0108')).toBe('010-8');
  });
});
