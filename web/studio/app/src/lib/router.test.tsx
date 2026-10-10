import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it } from 'vitest';
import { BrowserRouter, Link, Navigate, Route, Routes, matchPath, migrateHashUrl, toHref, useLocation, useParams, useSearchParams } from './router';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

describe('matchPath', () => {
  it('정적·파라미터·와일드카드', () => {
    expect(matchPath('/', '/')).toEqual({});
    expect(matchPath('/releases', '/releases/')).toEqual({});
    expect(matchPath('/releases', '/releases/r1')).toBeNull();
    expect(matchPath('/releases/:id', '/releases/r%201')).toEqual({ id: 'r 1' });
    expect(matchPath('/*', '/anything/deep')).toEqual({});
    expect(matchPath('*', '/x')).toEqual({});
    expect(matchPath('/', '/x')).toBeNull();
  });
});

function Detail() {
  const { id } = useParams();
  const [q, setQ] = useSearchParams();
  const loc = useLocation();
  return (
    <div>
      <span id="id">{id}</span><span id="tab">{q.get('tab') ?? ''}</span><span id="state">{String((loc.state as { from?: string } | null)?.from ?? '')}</span>
      <button id="set" onClick={() => setQ({ tab: 'tracks' }, { replace: true })}>set</button>
    </div>
  );
}

describe('BrowserRouter', () => {
  let host: HTMLDivElement;
  afterEach(() => { host?.remove(); window.history.replaceState(null, '', '/'); });

  it('링크 이동, 파라미터, 검색어, 리다이렉트 상태', async () => {
    window.history.replaceState(null, '', '/old');
    host = document.createElement('div');
    document.body.append(host);
    const root = createRoot(host);
    await act(async () => {
      root.render(
        <BrowserRouter>
          <Routes>
            <Route path="/" element={<Link id="go" to="/releases/r9">go</Link>} />
            <Route path="/releases/:id" element={<Detail />} />
            <Route path="*" element={<Navigate to="/" replace state={{ from: 'old' }} />} />
          </Routes>
        </BrowserRouter>,
      );
    });
    expect(window.location.pathname).toBe('/');
    await act(async () => { host.querySelector<HTMLAnchorElement>('#go')!.click(); });
    expect(window.location.pathname).toBe('/releases/r9');
    expect(host.querySelector('#go')).toBeNull();
    expect(host.querySelector('#id')!.textContent).toBe('r9');
    await act(async () => { host.querySelector<HTMLButtonElement>('#set')!.click(); });
    expect(window.location.pathname + window.location.search).toBe('/releases/r9?tab=tracks');
    expect(host.querySelector('#tab')!.textContent).toBe('tracks');
    act(() => root.unmount());
  });
});

describe('예전 해시 주소', () => {
  it('/#/login?x=1 → /login?x=1', () => {
    window.history.replaceState(null, '', '/#/login?x=1');
    migrateHashUrl();
    expect(window.location.pathname + window.location.search + window.location.hash).toBe('/login?x=1');
    window.history.replaceState(null, '', '/');
  });
  it('일반 앵커 해시는 그대로 둔다', () => {
    window.history.replaceState(null, '', '/help#main');
    migrateHashUrl();
    expect(window.location.pathname + window.location.hash).toBe('/help#main');
    window.history.replaceState(null, '', '/');
  });
  it('링크 href는 실제 경로', () => {
    expect(toHref('/releases/r1')).toBe('/releases/r1');
  });
});
