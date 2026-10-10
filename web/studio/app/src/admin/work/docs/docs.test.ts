import { describe, expect, it, vi } from 'vitest';
import { ApiError } from '../../../api/errors';
import type { DocNode, EmbedData } from '../types';
import { Autosave } from './autosave';
import { diffLines, diffStats } from './diff';
import { EmbedStore } from './embeds';
import { embedKeys, isSafeUrl, normalizeDoc, plainText } from './model';
import { filterSlash } from './slash';

/** 손으로 돌리는 타이머 — 자동 저장의 시간 흐름을 정확히 시험한다 */
function clock() {
  let now = 0;
  let seq = 0;
  const jobs = new Map<number, { at: number; fn: () => void }>();
  return {
    timers: {
      set: (fn: () => void, ms: number) => { const id = ++seq; jobs.set(id, { at: now + ms, fn }); return id; },
      clear: (h: unknown) => { jobs.delete(h as number); },
      now: () => now,
    },
    async tick(ms: number) {
      now += ms;
      for (const [id, j] of [...jobs].sort((a, b) => a[1].at - b[1].at)) {
        if (j.at <= now) { jobs.delete(id); j.fn(); }
      }
      for (let i = 0; i < 10; i++) await Promise.resolve();
    },
    pending: () => jobs.size,
  };
}

describe('자동 저장', () => {
  it('입력을 묶어 한 번만 보내고 버전을 이어 간다', async () => {
    const c = clock();
    const save = vi.fn(async (v: string, base: number) => ({ version: base + 1, v }));
    const a = new Autosave<string>({ version: 3, save, debounceMs: 1000, timers: c.timers });
    a.change('a'); a.change('ab'); a.change('abc');
    expect(a.snapshot.state).toBe('dirty');
    await c.tick(999);
    expect(save).not.toHaveBeenCalled();
    await c.tick(1);
    expect(save).toHaveBeenCalledTimes(1);
    expect(save).toHaveBeenLastCalledWith('abc', 3);
    expect(a.snapshot.state).toBe('saved');
    expect(a.version).toBe(4);
    a.change('abcd');
    await c.tick(1000);
    expect(save).toHaveBeenLastCalledWith('abcd', 4);
  });

  it('오래 쓰는 중이면 최대 대기 시간마다 저장한다', async () => {
    const c = clock();
    const save = vi.fn(async (_: number, base: number) => ({ version: base + 1 }));
    const a = new Autosave<number>({ version: 1, save, debounceMs: 1000, maxWaitMs: 3000, timers: c.timers });
    for (let i = 0; i < 10; i++) { a.change(i); await c.tick(500); }
    // 5초 동안 쉬지 않고 썼다: 3초 무렵 한 번은 저장됐어야 한다
    expect(save.mock.calls.length).toBeGreaterThanOrEqual(1);
    expect(save.mock.calls[0][0]).toBeLessThan(9);
  });

  it('네트워크 실패는 다시 시도하고, 그동안의 입력도 함께 보낸다', async () => {
    const c = clock();
    let fail = true;
    const save = vi.fn(async (_: string, base: number) => { if (fail) throw new Error('offline'); return { version: base + 1 }; });
    const a = new Autosave<string>({ version: 1, save, debounceMs: 100, retryMs: [2000], timers: c.timers });
    a.change('x');
    await c.tick(100);
    expect(a.snapshot.state).toBe('error');
    expect(a.unsaved).toBe(true);
    a.change('xy');
    fail = false;
    await c.tick(2000);
    expect(save).toHaveBeenLastCalledWith('xy', 1);
    expect(a.snapshot.state).toBe('saved');
    expect(a.unsaved).toBe(false);
  });

  it('재시도 버튼은 기다리지 않고 바로 보낸다', async () => {
    const c = clock();
    let fail = true;
    const save = vi.fn(async (_: string, base: number) => { if (fail) throw new Error('offline'); return { version: base + 1 }; });
    const a = new Autosave<string>({ version: 1, save, debounceMs: 100, retryMs: [60_000], timers: c.timers });
    a.change('x');
    await c.tick(100);
    fail = false;
    a.retry();
    for (let i = 0; i < 10; i++) await Promise.resolve();
    expect(a.snapshot.state).toBe('saved');
  });

  it('다른 사람이 먼저 고치면(409) 덮어쓰지 않고 멈춘다', async () => {
    const c = clock();
    const save = vi.fn(async () => { throw new ApiError('conflict', 409, 'CONFLICT'); });
    const a = new Autosave<string>({ version: 5, save, debounceMs: 100, timers: c.timers, isConflict: e => e instanceof ApiError && e.status === 409 });
    a.change('mine');
    await c.tick(100);
    expect(a.snapshot.state).toBe('conflict');
    a.change('more');
    await c.tick(10_000);
    expect(save).toHaveBeenCalledTimes(1);
    // 최신 버전으로 다시 시작하면 내 내용을 보낼 수 있다
    a.reset(7);
    a.change('mine');
    await a.flush();
    expect(save).toHaveBeenLastCalledWith('mine', 7);
  });

  it('저장 중 바뀐 내용은 끝난 뒤 이어서 보낸다 (동시에 두 번 보내지 않음)', async () => {
    const c = clock();
    let release: () => void = () => {};
    let inflight = 0;
    let max = 0;
    const save = vi.fn((_: string, base: number) => new Promise<{ version: number }>(r => {
      inflight++; max = Math.max(max, inflight);
      release = () => { inflight--; r({ version: base + 1 }); };
    }));
    const a = new Autosave<string>({ version: 1, save, debounceMs: 100, timers: c.timers });
    a.change('a');
    await c.tick(100);
    a.change('ab');
    await c.tick(100);
    expect(save).toHaveBeenCalledTimes(1);
    release();
    await c.tick(0);
    await c.tick(100);
    expect(save).toHaveBeenCalledTimes(2);
    expect(save).toHaveBeenLastCalledWith('ab', 2);
    release();
    await c.tick(0);
    expect(max).toBe(1);
  });
});

describe('문서 내용 정리', () => {
  const doc: DocNode = {
    type: 'doc', content: [
      { type: 'heading', attrs: { level: 2, id: 'x' }, content: [{ type: 'text', text: '제목' }] },
      { type: 'paragraph', content: [
        { type: 'text', text: '링크', marks: [{ type: 'link', attrs: { href: 'https://audeniq.com', target: '_blank', title: 't', class: null } }] },
        { type: 'text', text: '나쁜 링크', marks: [{ type: 'link', attrs: { href: 'javascript:alert(1)' } }, { type: 'bold' }] },
        { type: 'hardBreak', marks: [{ type: 'bold' }] } as DocNode,
      ] },
      { type: 'script', content: [{ type: 'text', text: 'alert(1)' }] },
      { type: 'entityEmbed', attrs: { kind: 'release', id: 'r1', view: 'qc', label: '새벽', extra: 1 } },
    ],
  };
  it('서버 허용 속성·마크만 남긴다', () => {
    const n = normalizeDoc(doc);
    const json = JSON.stringify(n);
    expect(json).not.toContain('javascript:');
    expect(json).not.toContain('"title":"t"');
    expect(json).not.toContain('"extra"');
    expect(json).not.toContain('"id":"x"');
    expect(json).not.toContain('"script"');
    const para = n.content?.[1];
    expect(para?.content?.[1]).toEqual({ type: 'text', text: '나쁜 링크', marks: [{ type: 'bold' }] });
    expect(para?.content?.[2]).toEqual({ type: 'hardBreak' });
    // 모르는 블록은 글자만 남긴 문단
    expect(n.content?.[2]).toEqual({ type: 'paragraph', content: [{ type: 'text', text: 'alert(1)' }] });
  });
  it('글자·임베드 키', () => {
    expect(plainText(doc)).toContain('제목');
    expect(embedKeys(doc)).toEqual(['release:r1:qc']);
  });
  it('안전한 주소만', () => {
    expect(isSafeUrl('https://a.b/c?d')).toBe(true);
    expect(isSafeUrl('mailto:a@b.c')).toBe(true);
    for (const u of ['javascript:alert(1)', 'data:text/html,x', '/relative', 'https://', 'vbscript:x']) expect(isSafeUrl(u)).toBe(false);
  });
});

describe('버전 비교', () => {
  it('줄 단위로 더한 줄·뺀 줄', () => {
    const d = diffLines('a\nb\nc', 'a\nB\nc\nd');
    expect(d.map(l => `${l.kind}:${l.text}`)).toEqual(['same:a', 'del:b', 'add:B', 'same:c', 'add:d']);
    expect(diffStats(d)).toEqual({ added: 2, removed: 1 });
    expect(diffLines('x', 'x')).toEqual([{ kind: 'same', text: 'x' }]);
  });
});

describe("'/' 명령", () => {
  it('이름·별칭으로 거른다', () => {
    expect(filterSlash('h2')[0].id).toBe('h2');
    expect(filterSlash('qc')[0].id).toBe('qc');
    expect(filterSlash('티켓')[0].id).toBe('ticket');
    expect(filterSlash('없는명령')).toEqual([]);
    expect(filterSlash('').length).toBeGreaterThan(20);
  });
});

describe('임베드 일괄 조회', () => {
  it('같은 순간의 블록은 한 요청으로, 다시 그려도 다시 부르지 않는다', async () => {
    const fetcher = vi.fn(async (keys: string[]) => Object.fromEntries(keys.map(k => [k, { kind: k.split(':')[0], id: k, view: 'summary' } as EmbedData])));
    const s = new EmbedStore(fetcher, 60_000, 0);
    s.want('release:a'); s.want('task:b'); s.want('release:a');
    await s.flush();
    expect(fetcher).toHaveBeenCalledTimes(1);
    expect(fetcher.mock.calls[0][0]).toEqual(['release:a', 'task:b']);
    expect(s.get('task:b')?.data?.id).toBe('task:b');
    s.want('release:a');
    await s.flush();
    expect(fetcher).toHaveBeenCalledTimes(1);
    s.want('release:a', true);
    await s.flush();
    expect(fetcher).toHaveBeenCalledTimes(2);
  });
  it('응답에 없는 키는 없음, 실패는 오류로 표시', async () => {
    const s = new EmbedStore(async () => ({}), 60_000, 0);
    s.want('doc:x');
    await s.flush();
    expect(s.get('doc:x')?.data?.missing).toBe(true);
    const bad = new EmbedStore(async () => { throw new Error('down'); }, 60_000, 0);
    bad.want('doc:y');
    await bad.flush();
    expect(bad.get('doc:y')?.error).toBe('down');
  });
});
