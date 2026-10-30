// @vitest-environment jsdom
import { describe, expect, it, vi } from 'vitest';
import { settleViewport } from './keyboardSettle';

// 문서 높이 2000, 화면 800 — 스크롤 끝은 1200
const fakeWindow = (scrollY: number, offsetTop: number) => {
  const scrollTo = vi.fn();
  const win = {
    scrollX: 0, scrollY, innerHeight: 800, scrollTo,
    visualViewport: { offsetTop },
    document: { scrollingElement: { scrollHeight: 2000 }, documentElement: { scrollHeight: 2000 } },
  } as unknown as Window;
  return { win, scrollTo };
};

describe('키보드가 내려간 뒤 화면 맞추기', () => {
  it('보이는 화면이 페이지 끝 너머로 밀려 있으면 끝으로 되돌린다', () => {
    const { win, scrollTo } = fakeWindow(1150, 340);
    expect(settleViewport(win)).toBe(true);
    expect(scrollTo).toHaveBeenLastCalledWith(0, 1200);
  });
  it('이미 끝에 있는 것으로 알려도 한 칸 옮겼다가 제자리로 와 사파리가 다시 그리게 한다', () => {
    const { win, scrollTo } = fakeWindow(1200, 340);
    expect(settleViewport(win)).toBe(true);
    expect(scrollTo.mock.calls).toEqual([[0, 1199], [0, 1200]]);
  });
  it('밀려 있지 않으면 건드리지 않는다', () => {
    const { win, scrollTo } = fakeWindow(600, 0);
    expect(settleViewport(win)).toBe(false);
    expect(scrollTo).not.toHaveBeenCalled();
  });
});
