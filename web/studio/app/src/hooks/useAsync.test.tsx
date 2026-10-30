// @vitest-environment jsdom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { useAsync } from './useAsync';
import { FRESH_MS, clearAsyncCache } from '../lib/asyncCache';
import { req, setCsrf, setOrgId } from '../api/http';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function Probe({ fn, k, out }: { fn: () => Promise<string>; k?: string; out: string[] }) {
  const { data, loading } = useAsync(fn, [], { key: k });
  out.push(`${loading ? 'loading' : 'ready'}:${data ?? '-'}`);
  return null;
}

async function mount(el: React.ReactElement) {
  const host = document.createElement('div');
  const root = createRoot(host);
  await act(async () => { root.render(el); });
  await act(async () => { await Promise.resolve(); });
  return () => act(() => root.unmount());
}

describe('useAsync 캐시', () => {
  afterEach(() => { clearAsyncCache(); setOrgId(''); vi.unstubAllGlobals(); });

  it('key가 있으면 다시 들어왔을 때 받아 둔 데이터를 바로 보여 주고 뒤에서 새로 받는다', async () => {
    setOrgId('org-1');
    let n = 0;
    const fn = () => Promise.resolve(`v${++n}`);
    const first: string[] = [];
    (await mount(<Probe fn={fn} k="list" out={first} />))();
    expect(first[0]).toBe('loading:-');
    expect(first.at(-1)).toBe('ready:v1');

    // 방금 받은 데이터면 다시 받지 않는다
    const soon: string[] = [];
    (await mount(<Probe fn={fn} k="list" out={soon} />))();
    expect(soon[0]).toBe('ready:v1');
    expect(soon.at(-1)).toBe('ready:v1');
    expect(n).toBe(1);

    // 시간이 지나 다시 들어오면 받아 둔 데이터를 먼저 보여 주고 새로 받는다
    const now = Date.now();
    vi.spyOn(Date, 'now').mockReturnValue(now + FRESH_MS + 1);
    const later: string[] = [];
    (await mount(<Probe fn={fn} k="list" out={later} />))();
    expect(later[0]).toBe('ready:v1');
    expect(later.at(-1)).toBe('ready:v2');
    vi.restoreAllMocks();
  });

  it('작업 공간이 바뀌면 앞 작업 공간의 데이터를 보여 주지 않는다', async () => {
    setOrgId('org-1');
    (await mount(<Probe fn={() => Promise.resolve('org1-data')} k="list" out={[]} />))();
    setOrgId('org-2');
    const out: string[] = [];
    (await mount(<Probe fn={() => Promise.resolve('org2-data')} k="list" out={out} />))();
    expect(out[0]).toBe('loading:-');
    expect(out).not.toContain('ready:org1-data');
  });

  it('같은 key를 쓰는 다른 곳이 새로 받으면 그 데이터로 바뀐다', async () => {
    setOrgId('org-1');
    const menu: string[] = [];
    const unmountMenu = await mount(<Probe fn={() => Promise.resolve('old')} k="list" out={menu} />);
    expect(menu.at(-1)).toBe('ready:old');
    // 다른 화면이 시간이 지난 뒤 같은 목록을 새로 받는다
    const now = Date.now();
    vi.spyOn(Date, 'now').mockReturnValue(now + FRESH_MS + 1);
    (await mount(<Probe fn={() => Promise.resolve('new')} k="list" out={[]} />))();
    vi.restoreAllMocks();
    expect(menu.at(-1)).toBe('ready:new');
    unmountMenu();
  });

  it('key가 없으면 예전처럼 매번 새로 받는다', async () => {
    const fn = () => Promise.resolve('x');
    (await mount(<Probe fn={fn} out={[]} />))();
    const out: string[] = [];
    (await mount(<Probe fn={fn} out={out} />))();
    expect(out[0]).toBe('loading:-');
  });
});

describe('req', () => {
  afterEach(() => { clearAsyncCache(); vi.unstubAllGlobals(); });

  it('같은 GET이 동시에 가면 한 번만 보낸다', async () => {
    const fetcher = vi.fn(async () => new Response(JSON.stringify({ ok: 1 })));
    vi.stubGlobal('fetch', fetcher);
    const [a, b] = await Promise.all([req('/api/x'), req('/api/x')]);
    expect(fetcher).toHaveBeenCalledTimes(1);
    expect(a).toEqual({ ok: 1 });
    expect(b).toEqual({ ok: 1 });
    // 끝난 뒤에는 다시 보낸다
    await req('/api/x');
    expect(fetcher).toHaveBeenCalledTimes(2);
  });

  it('바꾸는 요청이 끝나면 화면 데이터 캐시를 비운다', async () => {
    setOrgId('org-1');
    let n = 0;
    (await mount(<Probe fn={() => Promise.resolve(`v${++n}`)} k="list" out={[]} />))();
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{}')));
    setCsrf('t');
    await req('/api/y', { method: 'POST' });
    const out: string[] = [];
    (await mount(<Probe fn={() => Promise.resolve('fresh')} k="list" out={out} />))();
    expect(out[0]).toBe('loading:-');
    setOrgId('');
  });

  it('CSRF 토큰 받기는 화면 데이터 캐시를 비우지 않는다', async () => {
    setOrgId('org-1');
    (await mount(<Probe fn={() => Promise.resolve('kept')} k="list" out={[]} />))();
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ csrf_token: 't' }))));
    await req('/api/auth/csrf', { method: 'POST' });
    const out: string[] = [];
    (await mount(<Probe fn={() => Promise.resolve('other')} k="list" out={out} />))();
    expect(out[0]).toBe('ready:kept');
    setOrgId('');
  });
});

describe('useAsync 늦게 온 응답', () => {
  afterEach(() => { clearAsyncCache(); setOrgId(''); });

  it('먼저 보낸 요청이 나중에 끝나도 새 응답을 덮어쓰지 않는다 (reload 연달아)', async () => {
    setOrgId('org-1');
    const resolvers: ((v: string) => void)[] = [];
    const fn = () => new Promise<string>(r => { resolvers.push(r); });
    let reload = () => {};
    const seen: string[] = [];
    function R() {
      const s = useAsync(fn, [], { key: 'race' });
      reload = s.reload;
      seen.push(s.data ?? '-');
      return null;
    }
    const unmount = await mount(<R />);
    await act(async () => { reload(); });
    expect(resolvers.length).toBe(2);
    // 두 번째(새) 요청이 먼저 끝나고, 첫 번째(옛) 요청이 늦게 끝난다
    await act(async () => { resolvers[1]('new'); await Promise.resolve(); });
    await act(async () => { resolvers[0]('old'); await Promise.resolve(); });
    expect(seen.at(-1)).toBe('new');
    unmount();
  });
});

describe('useAsync 같은 key를 함께 부를 때', () => {
  afterEach(() => { clearAsyncCache(); setOrgId(''); });

  it('먼저 부른 화면도 자기 응답을 받아 빈 화면으로 남지 않는다', async () => {
    setOrgId('org-1');
    const resolvers: ((v: string) => void)[] = [];
    const fn = () => new Promise<string>(r => { resolvers.push(r); });
    const a: string[] = [];
    const b: string[] = [];
    function Two() { return <><Probe fn={fn} k="both" out={a} /><Probe fn={fn} k="both" out={b} /></>; }
    const unmount = await mount(<Two />);
    expect(resolvers.length).toBe(2);
    await act(async () => { resolvers[0]('a'); await Promise.resolve(); });
    expect(a.at(-1)).toBe('ready:a');
    await act(async () => { resolvers[1]('b'); await Promise.resolve(); });
    expect(a.at(-1)).toBe('ready:b');
    expect(b.at(-1)).toBe('ready:b');
    unmount();
  });
});
