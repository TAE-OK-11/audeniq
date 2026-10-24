import { describe, expect, it } from 'vitest';
import { artistPressPath, handleProblem, normalizeHandle, releasePath } from './publicLinks';

describe('공개 페이지 주소', () => {
  it('새 형식', () => {
    expect(releasePath('abcd2345')).toBe('/link/release/abcd2345');
    expect(artistPressPath('serin_official')).toBe('/link/artist/serin_official/press');
  });
  it('주소 이름 규칙 (서버와 같게)', () => {
    expect(normalizeHandle(' @Serin_Official ')).toBe('serin_official');
    expect(handleProblem('serin-2nd')).toBeNull();
    expect(handleProblem('ab')).not.toBeNull();
    expect(handleProblem('serin.music')).not.toBeNull();
    expect(handleProblem('-serin')).not.toBeNull();
    expect(handleProblem('a'.repeat(31))).not.toBeNull();
  });
});
