import { describe, expect, it } from 'vitest';
import { addCredit, creditChips, creditHistory, creditParts } from './credits';

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

describe('크레딧 자동완성', () => {
  it('자주 쓴 이름 순, 같으면 최근 것 먼저', () => {
    const h = creditHistory([
      { composers: 'B, C', lyricists: 'B' },
      { composers: 'A, B', lyricists: null },
      { composers: 'A', arrangers: 'D' },
    ]);
    expect(h.composers).toEqual(['B', 'A', 'C']);
    expect(h.lyricists).toEqual(['B']);
    expect(h.arrangers).toEqual(['D']);
  });
  it('이미 있는 이름은 빼고, 덧붙일 때는 쉼표로', () => {
    expect(creditChips(['서린', 'Joseph K', 'Trinity'], 'joseph k')).toEqual(['서린', 'Trinity']);
    expect(addCredit('', '서린')).toBe('서린');
    expect(addCredit('서린,', 'Joseph K')).toBe('서린, Joseph K');
  });
});
