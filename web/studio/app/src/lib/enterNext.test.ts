// @vitest-environment jsdom
import { afterEach, beforeAll, describe, expect, it } from 'vitest';
import { initEnterNext } from './enterNext';

const enter = (el: Element, init: KeyboardEventInit = {}) => {
  const e = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true, ...init });
  el.dispatchEvent(e);
  return e;
};
const focus = (id: string) => {
  const el = document.getElementById(id)!;
  el.focus();
  return el;
};

describe('키보드 다음 칸', () => {
  beforeAll(() => {
    initEnterNext();
    // jsdom은 크기를 재지 않는다 — 보이는 칸으로 친다
    Element.prototype.getBoundingClientRect = () => ({ width: 100, height: 40, top: 0, left: 0, right: 100, bottom: 40, x: 0, y: 0, toJSON() {} });
  });
  afterEach(() => { document.body.innerHTML = ''; });

  it('다음 칸이 있으면 ‘다음’, Enter로 그 칸으로, 마지막 칸은 ‘완료’로 키보드를 내린다', () => {
    document.body.innerHTML = '<div id="main"><input id="a"><select id="s"></select><textarea id="b"></textarea><input id="c"></div>';
    const a = focus('a');
    expect(a.getAttribute('enterkeyhint')).toBe('next');
    expect(enter(a).defaultPrevented).toBe(true);
    expect(document.activeElement?.id).toBe('b');
    const c = focus('c');
    expect(c.getAttribute('enterkeyhint')).toBe('done');
    enter(c);
    expect(document.activeElement).toBe(document.body);
  });

  it('폼 안의 칸·한글 조합 중·칸이 직접 처리한 Enter는 건드리지 않는다', () => {
    document.body.innerHTML = '<div id="main"><form><input id="f"><input id="g"></form><input id="a"><input id="b"></div>';
    const f = focus('f');
    expect(f.hasAttribute('enterkeyhint')).toBe(false);
    expect(enter(f).defaultPrevented).toBe(false);
    const a = focus('a');
    expect(enter(a, { isComposing: true }).defaultPrevented).toBe(false);
    expect(document.activeElement?.id).toBe('a');
    a.addEventListener('keydown', e => e.preventDefault(), { once: true });
    enter(a);
    expect(document.activeElement?.id).toBe('a');
  });

  it('칸이 정해 둔 enterkeyhint는 그대로 둔다', () => {
    document.body.innerHTML = '<div id="main"><input id="a" enterkeyhint="search"><input id="b"></div>';
    expect(focus('a').getAttribute('enterkeyhint')).toBe('search');
  });
});
