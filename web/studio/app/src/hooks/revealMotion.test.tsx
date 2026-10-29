// @vitest-environment jsdom
import { act, useRef } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { usePublicReveal } from './usePublicReveal';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function Screen({ kind }: { kind: string }) {
  const ref = useRef<HTMLDivElement>(null);
  usePublicReveal(ref, '.unit', kind === 'public');
  return <div id="main"><div ref={ref} className="view" id="view-test">
    <div className="unit ui-card"><button>visible</button></div>
    <div className="unit ui-card" data-below><button>below</button></div>
  </div></div>;
}

let root: Root;
let host: HTMLDivElement;
let query: EventTarget & { matches: boolean };
let animations: { el: Element; cancel: ReturnType<typeof vi.fn>; options: KeyframeAnimationOptions }[];

beforeEach(() => {
  query = Object.assign(new EventTarget(), { matches: false });
  vi.stubGlobal('matchMedia', () => query);
  vi.stubGlobal('IntersectionObserver', class {
    observe() {} unobserve() {} disconnect() {}
  });
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
    const top = this.hasAttribute('data-below') ? 2000 : 10;
    return { top, bottom: top + 100, width: 100, height: 100, left: 0, right: 100, x: 0, y: top, toJSON() {} };
  });
  animations = [];
  Object.defineProperty(Element.prototype, 'animate', { configurable: true, writable: true, value: vi.fn(function (this: Element, _frames, options) {
    const cancel = vi.fn();
    animations.push({ el: this, cancel, options: options as KeyframeAnimationOptions });
    return { cancel, finished: new Promise(() => {}) } as unknown as Animation;
  }) });
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  Reflect.deleteProperty(Element.prototype, 'animate');
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe.each(['public'])('%s reveal', kind => {
  it('실행 중 모션 축소를 켜면 진행 중 연출과 화면 밖 대기를 즉시 해제한다', () => {
    act(() => root.render(<Screen kind={kind} />));
    expect(animations.length).toBeGreaterThan(0);
    expect(host.querySelector('[data-below]')?.className).toContain('waiting');
    act(() => { query.matches = true; query.dispatchEvent(new Event('change')); });
    expect(animations.every(a => a.cancel.mock.calls.length === 1)).toBe(true);
    expect(host.querySelector('[class*="waiting"]')).toBeNull();
  });

  it('화면을 떠나면 진행 중인 애니메이션을 취소한다', () => {
    act(() => root.render(<Screen kind={kind} />));
    const running = [...animations];
    act(() => root.render(null));
    expect(running.length).toBeGreaterThan(0);
    expect(running.every(a => a.cancel.mock.calls.length === 1)).toBe(true);
  });

  it('모션 축소 설정으로 시작하면 숨김이나 애니메이션이 없다', () => {
    query.matches = true;
    act(() => root.render(<Screen kind={kind} />));
    expect(animations).toHaveLength(0);
    expect(host.querySelector('[class*="waiting"]')).toBeNull();
  });
});

it('공개 화면의 등장 중 버튼에 초점을 두면 해당 연출을 즉시 취소한다', () => {
  act(() => root.render(<Screen kind="public" />));
  const first = animations[0];
  act(() => host.querySelector('button')?.focus());
  expect(first.cancel).toHaveBeenCalledOnce();
});
