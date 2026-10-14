// @vitest-environment jsdom
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { initPressFeedback } from './press';

const fire = (el: Element, type: string, x = 0, y = 0) =>
  el.dispatchEvent(Object.assign(new Event(type, { bubbles: true }), { button: 0, clientX: x, clientY: y }));

describe('누름 피드백', () => {
  beforeAll(() => initPressFeedback());
  afterEach(() => { document.body.innerHTML = ''; vi.useRealTimers(); });

  it('누르는 순간 눌린 모습이 되고, 짧은 탭이어도 최소 시간 뒤에 풀린다', () => {
    vi.useFakeTimers();
    document.body.innerHTML = '<button class="ui-btn"><span>보내기</span></button>';
    const btn = document.querySelector('button')!;
    fire(btn.querySelector('span')!, 'pointerdown');
    expect(btn.classList.contains('is-pressed')).toBe(true);
    fire(btn, 'pointerup');
    expect(btn.classList.contains('is-pressed')).toBe(true);
    vi.advanceTimersByTime(150);
    expect(btn.classList.contains('is-pressed')).toBe(false);
  });

  it('손가락이 움직이면(스크롤) 바로 푼다', () => {
    document.body.innerHTML = '<a href="/x" class="ui-row">줄</a>';
    const row = document.querySelector('a')!;
    fire(row, 'pointerdown', 10, 10);
    fire(row, 'pointermove', 10, 40);
    expect(row.classList.contains('is-pressed')).toBe(false);
  });

  it('비활성 버튼과 누를 수 없는 곳은 건드리지 않는다', () => {
    document.body.innerHTML = '<button disabled>x</button><p>글</p>';
    fire(document.querySelector('button')!, 'pointerdown');
    fire(document.querySelector('p')!, 'pointerdown');
    expect(document.querySelectorAll('.is-pressed').length).toBe(0);
  });
});
