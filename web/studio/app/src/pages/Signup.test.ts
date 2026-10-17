import { describe, expect, it } from 'vitest';
import { PASSWORD_MIN } from '../api/client';
import { strength } from './Signup';

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
