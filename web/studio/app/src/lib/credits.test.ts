import { describe, expect, it } from 'vitest';
import { creditParts } from './credits';

describe('트랙 크레딧', () => {
  it('작곡·작사가 같으면 한 번만', () => {
    expect(creditParts('A, B', 'B,A')).toEqual(['작사·작곡 A, B']);
  });
  it('다르면 따로, 비었으면 입력 전', () => {
    expect(creditParts('A', 'B')).toEqual(['작곡 A', '작사 B']);
    expect(creditParts('A', '')).toEqual(['작곡 A', '작사 입력 전']);
    expect(creditParts(null, null)).toEqual(['작곡 입력 전', '작사 입력 전']);
  });
});
