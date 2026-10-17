// @vitest-environment jsdom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { CountUp } from './CountUp';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

describe('CountUp', () => {
  afterEach(() => { document.body.innerHTML = ''; vi.unstubAllGlobals(); });

  it('값이 바뀌면 보이던 값에서 새 값 쪽으로만 움직인다 — 최종값이 먼저 번쩍이거나 반대쪽으로 튀지 않는다', () => {
    const frames: FrameRequestCallback[] = [];
    vi.stubGlobal('requestAnimationFrame', (cb: FrameRequestCallback) => { frames.push(cb); return frames.length; });
    vi.stubGlobal('cancelAnimationFrame', () => {});
    const host = document.createElement('div');
    document.body.appendChild(host);
    const root = createRoot(host);
    const run = (t: number) => { const cbs = frames.splice(0); cbs.forEach(cb => cb(t)); };
    act(() => root.render(<CountUp value={100} duration={100} />));
    run(0); run(200);
    expect(host.textContent).toBe('100');

    act(() => root.render(<CountUp value={50} duration={100} />));
    // 다음 프레임 전에는 아직 보이던 값
    expect(host.textContent).toBe('100');
    // rAF가 넘기는 시각은 효과를 건 시점보다 앞설 수 있다(브라우저의 프레임 시작 시각)
    const seen: number[] = [];
    for (const t of [1000, 1010, 1040, 1080, 1120]) { run(t); seen.push(Number(host.textContent)); }
    expect(seen.every(v => v <= 100 && v >= 50)).toBe(true);
    expect(seen.every((v, i) => i === 0 || v <= seen[i - 1])).toBe(true);
    expect(seen.at(-1)).toBe(50);
    act(() => root.unmount());
  });
});
