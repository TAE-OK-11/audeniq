import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it } from 'vitest';
import { HashRouter, Link, Navigate, Route, Routes, matchPath, useLocation, useParams, useSearchParams } from './router';

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

describe('HashRouter', () => {
  let host: HTMLDivElement;
  afterEach(() => { host?.remove(); window.location.hash = ''; });

  it('링크 이동, 파라미터, 검색어, 리다이렉트 상태', async () => {
    window.history.replaceState(null, '', '#/old');
    host = document.createElement('div');
    document.body.append(host);
    const root = createRoot(host);
    await act(async () => {
      root.render(
        <HashRouter>
          <Routes>
            <Route path="/" element={<Link id="go" to="/releases/r9">go</Link>} />
            <Route path="/releases/:id" element={<Detail />} />
            <Route path="*" element={<Navigate to="/" replace state={{ from: 'old' }} />} />
          </Routes>
        </HashRouter>,
      );
    });
    expect(window.location.hash).toBe('#/');
    await act(async () => { host.querySelector<HTMLAnchorElement>('#go')!.click(); });
    expect(window.location.hash).toBe('#/releases/r9');
    expect(host.querySelector('#id')!.textContent).toBe('r9');
    await act(async () => { host.querySelector<HTMLButtonElement>('#set')!.click(); });
    expect(window.location.hash).toBe('#/releases/r9?tab=tracks');
    expect(host.querySelector('#tab')!.textContent).toBe('tracks');
    act(() => root.unmount());
  });
});
