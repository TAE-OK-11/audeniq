import { describe, expect, it } from 'vitest';
import { promoKicker, wrapText } from './promoImage';

describe('홍보 이미지', () => {
  it('발매 전·당일·뒤 문구', () => {
    expect(promoKicker('2026-11-15', '2026-10-24')).toBe('11월 15일 발매 · D-22');
    expect(promoKicker('2026-10-24', '2026-10-24')).toBe('오늘 발매');
    expect(promoKicker('2026-10-01', '2026-10-24')).toBe('지금 들을 수 있어요');
    expect(promoKicker(null, '2026-10-24')).toBe('지금 들을 수 있어요');
  });
  it('폭에 맞춰 줄바꿈, 넘치면 …', () => {
    const m = (s: string) => s.length * 10;
    expect(wrapText(m, '여름 EP', 100, 2)).toEqual(['여름 EP']);
    expect(wrapText(m, 'aaaa bbbb cccc dddd', 100, 2)).toEqual(['aaaa bbbb', 'cccc dddd']);
    expect(wrapText(m, 'aaaa bbbb cccc dddd eeee', 100, 2)).toEqual(['aaaa bbbb', 'cccc dddd…']);
  });
});
