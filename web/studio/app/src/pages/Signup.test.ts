import { describe, expect, it } from 'vitest';
import { PASSWORD_MIN } from '../api/client';
import { formatPhone, isPhone, strength } from './Signup';

describe('strength', () => {
  it('최소 길이보다 짧으면 조합과 상관없이 너무 짧다', () => {
    expect(strength('a1!'.padEnd(PASSWORD_MIN - 1, 'x'))).toEqual({ score: 0, label: '너무 짧아요' });
  });
  it('길이를 채우면 조합에 따라 올라간다', () => {
    expect(strength('a'.repeat(PASSWORD_MIN)).score).toBe(1);
    expect(strength('a1'.padEnd(PASSWORD_MIN, 'b')).score).toBe(2);
    expect(strength('a1!'.padEnd(PASSWORD_MIN + 4, 'b')).score).toBe(4);
  });
});

describe('formatPhone', () => {
  it('입력하는 대로 하이픈을 넣는다', () => {
    expect(formatPhone('010')).toBe('010');
    expect(formatPhone('0101234')).toBe('010-1234');
    expect(formatPhone('01012345678')).toBe('010-1234-5678');
    expect(formatPhone('0161234567')).toBe('016-123-4567');
  });
  it('숫자만 남기고 11자리에서 자른다', () => {
    expect(formatPhone('010 1234 5678 9')).toBe('010-1234-5678');
  });
  it('휴대폰 번호인지 본다', () => {
    expect(isPhone('010-1234-5678')).toBe(true);
    expect(isPhone('02-123-4567')).toBe(false);
    expect(isPhone('010-123')).toBe(false);
    // 서버 규칙과 같이: 010은 11자리만, 011·016~019는 10자리도
    expect(isPhone('010-123-4567')).toBe(false);
    expect(isPhone('011-234-5678')).toBe(true);
    expect(isPhone('016-1234-5678')).toBe(true);
    expect(isPhone('015-1234-5678')).toBe(false);
  });
});
