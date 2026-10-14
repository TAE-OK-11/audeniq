// @vitest-environment jsdom
import { beforeAll, describe, expect, it } from 'vitest';
import { initRowNav } from './rowNav';

const key = (el: Element, k: string) => el.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true }));

describe('탭 줄 키보드', () => {
  beforeAll(() => {
    document.body.innerHTML = '<div id="root"></div>';
    initRowNav();
  });

  it('← → Home End로 탭을 옮겨 고른다', () => {
    const root = document.getElementById('root')!;
    root.innerHTML = '<div role="tablist"><button role="tab">A</button><button role="tab">B</button><button role="tab" disabled>C</button><button role="tab">D</button></div>';
    const tabs = [...root.querySelectorAll<HTMLButtonElement>('[role=tab]')];
    // jsdom은 레이아웃이 없어 offsetParent가 null — 보이는 것으로 친다
    tabs.forEach(t => Object.defineProperty(t, 'offsetParent', { get: () => root }));
    const clicked: string[] = [];
    tabs.forEach(t => t.addEventListener('click', () => clicked.push(t.textContent!)));
    tabs[0].focus();
    key(tabs[0], 'ArrowRight');
    expect(document.activeElement).toBe(tabs[1]);
    key(tabs[1], 'ArrowRight'); // 비활성 C를 건너뛴다
    expect(document.activeElement).toBe(tabs[3]);
    key(tabs[3], 'Home');
    expect(document.activeElement).toBe(tabs[0]);
    key(tabs[0], 'ArrowLeft'); // 처음에서 왼쪽은 끝으로
    expect(document.activeElement).toBe(tabs[3]);
    expect(clicked).toEqual(['B', 'D', 'A', 'D']);
  });

  it('탭이 아닌 곳의 화살표 키는 건드리지 않는다', () => {
    const root = document.getElementById('root')!;
    root.innerHTML = '<input id="q">';
    const ev = new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true, cancelable: true });
    root.querySelector('input')!.dispatchEvent(ev);
    expect(ev.defaultPrevented).toBe(false);
  });
});
