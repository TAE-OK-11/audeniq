import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { useSharpImage } from './ReleaseCover';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// jsdom은 이미지를 받지 않는다 — 만든 Image를 모아 두고 테스트가 직접 onload를 부른다
const made: { src: string; onload: (() => void) | null }[] = [];
const RealImage = globalThis.Image;
beforeEach(() => {
  made.length = 0;
  globalThis.Image = class { src = ''; decoding = ''; onload: (() => void) | null = null; constructor() { made.push(this); } } as unknown as typeof Image;
});
afterEach(() => { globalThis.Image = RealImage; document.body.innerHTML = ''; });

function Probe({ preview, full }: { preview?: string; full?: string }) {
  return <span>{useSharpImage(preview, full) ?? ''}</span>;
}

describe('useSharpImage', () => {
  it('원본을 다 받기 전엔 미리보기, 받은 뒤엔 원본', () => {
    const el = document.createElement('div');
    document.body.append(el);
    const root = createRoot(el);
    act(() => root.render(<Probe preview="thumb.jpg" full="full.jpg" />));
    expect(el.textContent).toBe('thumb.jpg');
    expect(made[0].src).toBe('full.jpg');
    act(() => made[0].onload?.());
    expect(el.textContent).toBe('full.jpg');
    act(() => root.unmount());
  });

  it('원본이 없으면 미리보기 그대로', () => {
    const el = document.createElement('div');
    document.body.append(el);
    const root = createRoot(el);
    act(() => root.render(<Probe preview="thumb.jpg" />));
    expect(el.textContent).toBe('thumb.jpg');
    expect(made).toHaveLength(0);
    act(() => root.unmount());
  });

  it('다른 발매로 바뀌면 이전 원본을 쓰지 않는다', () => {
    const el = document.createElement('div');
    document.body.append(el);
    const root = createRoot(el);
    act(() => root.render(<Probe preview="a-thumb.jpg" full="a.jpg" />));
    act(() => made[0].onload?.());
    act(() => root.render(<Probe preview="b-thumb.jpg" full="b.jpg" />));
    expect(el.textContent).toBe('b-thumb.jpg');
    act(() => made[0].onload?.()); // 늦게 도착한 이전 원본
    expect(el.textContent).toBe('b-thumb.jpg');
    act(() => made[1].onload?.());
    expect(el.textContent).toBe('b.jpg');
    act(() => root.unmount());
  });
});
