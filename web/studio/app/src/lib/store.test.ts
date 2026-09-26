import { beforeEach, describe, expect, it } from 'vitest';
import { createStore } from './store';

describe('createStore', () => {
  beforeEach(() => localStorage.clear());

  it('영속화하고 다시 불러온다', () => {
    const a = createStore<number[]>([], { persist: 't1' });
    a.set(v => [...v, 1, 2]);
    const b = createStore<number[]>([], { persist: 't1' });
    expect(b.get()).toEqual([1, 2]);
  });

  it('손상된 저장값이면 초기값으로 복구', () => {
    localStorage.setItem('aq.studio.v2.t2', '{broken');
    const s = createStore<string[]>(['seed'], { persist: 't2' });
    expect(s.get()).toEqual(['seed']);
  });

  it('serialize로 민감/직렬화 불가 값을 제외', () => {
    const s = createStore<{ a: number; secret?: string }>({ a: 1 }, {
      persist: 't3',
      serialize: ({ secret: _s, ...rest }) => rest,
    });
    s.set({ a: 2, secret: 'x' });
    expect(JSON.parse(localStorage.getItem('aq.studio.v2.t3')!)).toEqual({ a: 2 });
  });

  it('구독자에게 변경을 알린다', () => {
    const s = createStore(0);
    let calls = 0;
    const off = s.subscribe(() => calls++);
    s.set(1); s.set(1); s.set(2);
    off(); s.set(3);
    expect(calls).toBe(2);
  });
});
