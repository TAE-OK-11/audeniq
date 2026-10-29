// 머리만 오고 본문이 멈춘 응답도 제한 시간 안에 '응답이 늦어요'로 끝나는지
import { afterEach, describe, expect, it, vi } from 'vitest';
import { req } from './http';
import { ApiError } from './errors';

afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });

describe('req 제한 시간', () => {
  it('본문을 받다 멈추면 TIMEOUT으로 끝난다', async () => {
    vi.useFakeTimers();
    vi.stubGlobal('fetch', vi.fn(async (_: string, init: RequestInit) => ({
      ok: true,
      status: 200,
      // 본문은 오지 않다가, 요청이 중단되면 그때 실패한다 (브라우저와 같게)
      text: () => new Promise<string>((_, reject) => {
        init.signal?.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError')));
      }),
    })));
    const p = req('/api/slow-body', { timeoutMs: 1000 });
    const caught = p.catch(e => e);
    await vi.advanceTimersByTimeAsync(1001);
    const e = await caught;
    expect(e).toBeInstanceOf(ApiError);
    expect((e as ApiError).code).toBe('TIMEOUT');
  });
});
