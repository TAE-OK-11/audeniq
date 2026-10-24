import { describe, expect, it } from 'vitest';
import { qrMatrix } from './qr';

describe('QR', () => {
  it('정사각 행렬, 세 모서리에 위치 표시(7×7 테두리)', () => {
    const m = qrMatrix('https://studio.audeniq.com/l/abcd2345');
    const n = m.length;
    expect(m.every(r => r.length === n)).toBe(true);
    expect(n).toBeGreaterThanOrEqual(21);
    for (const [r, c] of [[0, 0], [0, n - 7], [n - 7, 0]]) {
      expect(m[r][c] && m[r][c + 6] && m[r + 6][c] && m[r + 6][c + 6]).toBe(true);
      expect(m[r + 1][c + 1]).toBe(false);
    }
  });
});
