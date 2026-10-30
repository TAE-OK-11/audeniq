import { afterEach, describe, expect, it } from 'vitest';
import { watchBusyButtons } from './busyButtons';

const flush = () => new Promise(r => setTimeout(r, 0));
let stop = () => {};
afterEach(() => { stop(); document.body.innerHTML = ''; });

describe('watchBusyButtons', () => {
  it('막힌 버튼의 글자가 …중이면 data-busy, 풀리면 뺀다', async () => {
    document.body.innerHTML = '<button class="ui-btn">저장</button>';
    stop = watchBusyButtons();
    const b = document.querySelector('button')!;
    b.disabled = true; b.textContent = '저장하는 중';
    await flush();
    expect(b.hasAttribute('data-busy')).toBe(true);
    b.disabled = false; b.textContent = '저장';
    await flush();
    expect(b.hasAttribute('data-busy')).toBe(false);
  });

  it('막히지 않은 버튼(필터 "진행 중")이나 그냥 막힌 버튼은 건드리지 않는다', async () => {
    document.body.innerHTML = '<button class="ui-btn">진행 중</button><button class="ui-btn" disabled>다음</button>';
    stop = watchBusyButtons();
    await flush();
    expect(document.querySelectorAll('[data-busy]')).toHaveLength(0);
  });

  it('새로 그려진 버튼도 본다', async () => {
    stop = watchBusyButtons();
    document.body.insertAdjacentHTML('beforeend', '<div><button class="ui-btn" disabled>보내는 중…</button></div>');
    await flush();
    expect(document.querySelector('button')!.hasAttribute('data-busy')).toBe(true);
  });
});
