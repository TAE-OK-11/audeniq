import { describe, expect, it } from 'vitest';
import { coverEdge } from './topCover';

describe('coverEdge', () => {
  it('키보드가 없으면 화면 맨 위(scrollY)까지 덮는다', () => {
    expect(coverEdge(1200, { offsetTop: 0 })).toBe(1200);
    expect(coverEdge(1200, null)).toBe(1200);
  });

  it('키보드가 떠서 보이는 화면만 내려가 있으면 그 맨 위까지 덮는다 — 상태 표시줄 칸 아래에 틈이 생기지 않게', () => {
    expect(coverEdge(1200, { offsetTop: 260 })).toBe(1460);
  });

  it('바운스 등으로 offsetTop이 음수여도 화면 맨 위보다 내려 덮지 않는다', () => {
    expect(coverEdge(0, { offsetTop: -30 })).toBe(0);
  });
});
