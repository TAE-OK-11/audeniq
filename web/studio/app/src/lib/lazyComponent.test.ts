// 화면 조각을 못 받았을 때 — 잠깐 끊긴 회선은 한 번 더 받아 보고, 오프라인이면 새로고침하지 않는다
import { afterEach, describe, expect, it, vi } from 'vitest';
import { lazyComponent } from './lazyComponent';

type Lazy = { _init: (p: unknown) => unknown; _payload: unknown };
/** React.lazy 내부 로더를 직접 불러 결과를 기다린다 */
async function resolveLazy(c: unknown) {
  const l = c as Lazy;
  try { l._init(l._payload); } catch (thenable) { await Promise.resolve(thenable).catch(() => {}); }
  return l._init(l._payload);
}

afterEach(() => { vi.restoreAllMocks(); vi.useRealTimers(); sessionStorage.clear(); });

describe('lazyComponent', () => {
  it('한 번 실패하면 잠시 뒤 다시 받아 그대로 연다', async () => {
    vi.useFakeTimers();
    const View = () => null;
    const load = vi.fn()
      .mockRejectedValueOnce(new TypeError('Failed to fetch dynamically imported module: /assets/a.js'))
      .mockResolvedValueOnce({ View });
    const C = lazyComponent(load, m => (m as { View: typeof View }).View);
    const done = resolveLazy(C);
    await vi.advanceTimersByTimeAsync(800);
    expect(await done).toBe(View);
    expect(load).toHaveBeenCalledTimes(2);
  });

  it('오프라인이면 새로고침하지 않고 오류를 그대로 넘긴다', async () => {
    vi.spyOn(navigator, 'onLine', 'get').mockReturnValue(false);
    const reload = vi.fn();
    vi.stubGlobal('location', { ...window.location, reload });
    const load = vi.fn().mockRejectedValue(new TypeError('Failed to fetch dynamically imported module: /assets/a.js'));
    const C = lazyComponent(load, () => () => null);
    await expect(resolveLazy(C)).rejects.toThrow(/dynamically imported module/);
    expect(load).toHaveBeenCalledTimes(1);
    expect(reload).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });
});
