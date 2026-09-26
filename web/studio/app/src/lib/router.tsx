// 경량 해시 라우터 — 이 앱이 쓰는 react-router API의 부분집합만 같은 이름·시그니처로 구현한다.
// (HashRouter, Routes, Route, Navigate, Link, useNavigate, useLocation, useParams, useSearchParams)
// react-router 전체(약 48KB min / 15KB gzip) 대신 2KB 남짓으로 같은 동작을 제공한다.
// 필요해지면 import 경로를 'react-router'로 바꾸는 것만으로 되돌릴 수 있다.
import {
  Children, createContext, isValidElement, startTransition, useCallback, useContext, useEffect, useMemo, useState,
  type AnchorHTMLAttributes, type MouseEvent, type ReactElement, type ReactNode,
} from 'react';

export interface Location {
  pathname: string;
  search: string;
  hash: string;
  state: unknown;
  key: string;
}

export interface NavigateOptions {
  replace?: boolean;
  state?: unknown;
}

export type NavigateFunction = (to: string | number, opts?: NavigateOptions) => void;

interface RouterCtx {
  location: Location;
  navigate: NavigateFunction;
}

const RouterContext = createContext<RouterCtx | null>(null);
const ParamsContext = createContext<Record<string, string>>({});

let keySeq = 0;
const nextKey = () => (++keySeq).toString(36) + Date.now().toString(36).slice(-4);

function readLocation(): Location {
  const raw = window.location.hash.replace(/^#/, '') || '/';
  const hashIdx = raw.indexOf('#');
  const noHash = hashIdx >= 0 ? raw.slice(0, hashIdx) : raw;
  const q = noHash.indexOf('?');
  const pathname = (q >= 0 ? noHash.slice(0, q) : noHash) || '/';
  const st = window.history.state as { usr?: unknown; key?: string } | null;
  return {
    pathname: pathname.startsWith('/') ? pathname : '/' + pathname,
    search: q >= 0 ? noHash.slice(q) : '',
    hash: hashIdx >= 0 ? raw.slice(hashIdx) : '',
    state: st?.usr ?? null,
    key: st?.key ?? 'default',
  };
}

export function HashRouter({ children }: { children: ReactNode }) {
  const [location, setLocation] = useState<Location>(() => {
    if (!window.location.hash) window.history.replaceState({ usr: null, key: 'default' }, '', '#/');
    return readLocation();
  });

  useEffect(() => {
    const sync = () => startTransition(() => setLocation(readLocation()));
    // popstate: 뒤로/앞으로 가기, hashchange: 주소창에서 해시를 직접 바꾼 경우
    window.addEventListener('popstate', sync);
    window.addEventListener('hashchange', sync);
    return () => {
      window.removeEventListener('popstate', sync);
      window.removeEventListener('hashchange', sync);
    };
  }, []);

  const navigate = useCallback<NavigateFunction>((to, opts = {}) => {
    if (typeof to === 'number') { window.history.go(to); return; }
    const target = '#' + (to.startsWith('/') ? to : '/' + to);
    const entry = { usr: opts.state ?? null, key: nextKey() };
    // 같은 주소로의 이동은 기록을 쌓지 않는다
    const same = window.location.hash === target;
    if (opts.replace || same) window.history.replaceState(entry, '', target);
    else window.history.pushState(entry, '', target);
    // 전환으로 처리해 다음 화면 청크를 받는 동안 현재 화면을 유지한다 (스켈레톤 깜빡임 방지)
    const next = readLocation();
    startTransition(() => setLocation(next));
  }, []);

  const value = useMemo(() => ({ location, navigate }), [location, navigate]);
  return <RouterContext.Provider value={value}>{children}</RouterContext.Provider>;
}

function useRouter(): RouterCtx {
  const ctx = useContext(RouterContext);
  if (!ctx) throw new Error('라우터 훅은 <HashRouter> 안에서만 쓸 수 있어요.');
  return ctx;
}

export const useLocation = () => useRouter().location;
export const useNavigate = () => useRouter().navigate;
export const useParams = <T extends Record<string, string> = Record<string, string>>() =>
  useContext(ParamsContext) as Partial<T>;

export function useSearchParams(): [URLSearchParams, (next: URLSearchParams | Record<string, string>, opts?: NavigateOptions) => void] {
  const { location, navigate } = useRouter();
  const params = useMemo(() => new URLSearchParams(location.search), [location.search]);
  const set = useCallback((next: URLSearchParams | Record<string, string>, opts?: NavigateOptions) => {
    const qs = new URLSearchParams(next as Record<string, string>).toString();
    navigate(location.pathname + (qs ? '?' + qs : ''), opts);
  }, [location.pathname, navigate]);
  return [params, set];
}

/** 경로 패턴 매칭 — '/a/:id', '/a/*', '*' 지원. 일치하면 파라미터 객체, 아니면 null */
export function matchPath(pattern: string, pathname: string): Record<string, string> | null {
  if (pattern === '*' || pattern === '/*') return {};
  const clean = (p: string) => p.replace(/\/+$/, '').split('/').filter(Boolean);
  const pat = clean(pattern);
  const seg = clean(pathname);
  const params: Record<string, string> = {};
  for (let i = 0; i < pat.length; i++) {
    const p = pat[i];
    if (p === '*') return params;
    const s = seg[i];
    if (s === undefined) return null;
    if (p.startsWith(':')) {
      try { params[p.slice(1)] = decodeURIComponent(s); } catch { params[p.slice(1)] = s; }
    } else if (p !== s) {
      return null;
    }
  }
  return seg.length === pat.length ? params : null;
}

interface RouteProps {
  path: string;
  element: ReactNode;
}

/** 설정용 컴포넌트 — 실제 렌더는 <Routes>가 담당 */
export function Route(_props: RouteProps): null {
  return null;
}

/** 자식 <Route> 중 처음으로 일치하는 것을 렌더 */
export function Routes({ children }: { children: ReactNode }) {
  const { location } = useRouter();
  let matched: { element: ReactNode; params: Record<string, string> } | null = null;
  Children.forEach(children, child => {
    if (matched || !isValidElement(child)) return;
    const { path, element } = (child as ReactElement<RouteProps>).props;
    const params = matchPath(path, location.pathname);
    if (params) matched = { element, params };
  });
  if (!matched) return null;
  const m = matched as { element: ReactNode; params: Record<string, string> };
  return <ParamsContext.Provider value={m.params}>{m.element}</ParamsContext.Provider>;
}

export function Navigate({ to, replace, state }: { to: string } & NavigateOptions) {
  const navigate = useNavigate();
  useEffect(() => { navigate(to, { replace, state }); }, [navigate, to, replace, state]);
  return null;
}

type LinkProps = Omit<AnchorHTMLAttributes<HTMLAnchorElement>, 'href'> & { to: string } & NavigateOptions;

export function Link({ to, replace, state, onClick, target, children, ...rest }: LinkProps) {
  const navigate = useNavigate();
  const handle = (e: MouseEvent<HTMLAnchorElement>) => {
    onClick?.(e);
    // 새 탭 열기(⌘/Ctrl/Shift 클릭, 가운데 버튼)는 브라우저 기본 동작에 맡긴다
    if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey || (target && target !== '_self')) return;
    e.preventDefault();
    navigate(to, { replace, state });
  };
  return <a {...rest} target={target} href={'#' + to} onClick={handle}>{children}</a>;
}
