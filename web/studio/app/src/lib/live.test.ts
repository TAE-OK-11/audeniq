// 실시간 연결 — 끊겼을 법한 순간(인터넷 복귀·오래 떠나 있다 돌아옴·잠들었다 깸)에 다시 붙는지
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { openLive } from './live';

class FakeES {
  static made: FakeES[] = [];
  readyState = 1;
  closed = false;
  onerror: (() => void) | null = null;
  listeners = new Map<string, (e: unknown) => void>();
  constructor(public url: string) { FakeES.made.push(this); }
  addEventListener(name: string, fn: (e: unknown) => void) { this.listeners.set(name, fn); }
  close() { this.closed = true; this.readyState = 2; }
}

beforeEach(() => { FakeES.made = []; vi.stubGlobal('EventSource', FakeES); vi.useFakeTimers(); });
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });

describe('openLive', () => {
  it('인터넷이 돌아오면 새로 연결하고 ready에서 따라잡는다', () => {
    const resync = vi.fn();
    const stop = openLive('/api/x/live', { on: {}, resync });
    expect(FakeES.made).toHaveLength(1);
    window.dispatchEvent(new Event('online'));
    expect(FakeES.made[0].closed).toBe(true);
    expect(FakeES.made).toHaveLength(2);
    FakeES.made[1].listeners.get('ready')?.({});
    expect(resync).toHaveBeenCalledTimes(1);
    stop();
    expect(FakeES.made[1].closed).toBe(true);
  });

  it('잠들었다 깨서 타이머가 크게 밀리면 다시 연결한다', () => {
    const stop = openLive('/api/x/live', { on: {}, resync: () => {} });
    vi.setSystemTime(Date.now() + 60_000); // 기기가 1분 잠듦
    vi.advanceTimersByTime(15_000);
    expect(FakeES.made).toHaveLength(2);
    vi.advanceTimersByTime(15_000); // 평소 간격이면 그대로
    expect(FakeES.made).toHaveLength(2);
    stop();
  });

  it('그만두면 다시 붙지 않는다', () => {
    const stop = openLive('/api/x/live', { on: {}, resync: () => {} });
    stop();
    window.dispatchEvent(new Event('online'));
    expect(FakeES.made).toHaveLength(1);
  });
});
