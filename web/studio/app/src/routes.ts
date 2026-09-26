// 라우트별 코드 스플리팅 로더 — App의 lazy()와 프리페치가 같은 로더를 공유한다.
// 같은 import()는 브라우저가 한 번만 받으므로 프리페치 후 실제 이동 시 즉시 렌더된다.
export const pageLoaders = {
  Login: () => import('./pages/Login'),
  Signup: () => import('./pages/Signup'),
  FindAccount: () => import('./pages/FindAccount'),
  Dashboard: () => import('./pages/Dashboard'),
  Releases: () => import('./pages/Releases'),
  ReleaseDetail: () => import('./pages/ReleaseDetail'),
  Upload: () => import('./pages/Upload'),
  Reports: () => import('./pages/Reports'),
  Settlement: () => import('./pages/Settlement'),
  Contracts: () => import('./pages/Contracts'),
  Rights: () => import('./pages/Rights'),
  Inquiries: () => import('./pages/Inquiries'),
  Notifications: () => import('./pages/Notifications'),
  Events: () => import('./pages/Events'),
  Notices: () => import('./pages/Notices'),
  Profile: () => import('./pages/Profile'),
};

type PageName = keyof typeof pageLoaders;

// 경로 → 그 화면에 필요한 청크
const ROUTE_PAGES: [RegExp, PageName[]][] = [
  [/^\/$/, ['Dashboard']],
  [/^\/releases\/?$/, ['Releases']],
  [/^\/releases\/.+/, ['ReleaseDetail']],
  [/^\/upload/, ['Upload']],
  [/^\/reports/, ['Reports']],
  [/^\/settlement/, ['Settlement']],
  [/^\/contracts/, ['Contracts']],
  [/^\/rights/, ['Rights']],
  [/^\/inquiries/, ['Inquiries']],
  [/^\/notifications/, ['Notifications']],
  [/^\/events/, ['Events']],
  [/^\/notices/, ['Notices']],
  [/^\/profile/, ['Profile']],
];

const requested = new Set<PageName>();

function load(name: PageName) {
  if (requested.has(name)) return;
  requested.add(name);
  pageLoaders[name]().catch(() => requested.delete(name)); // 실패하면 다음에 다시 시도
}

/** 메뉴 호버·포커스 시 해당 화면 청크를 미리 받는다 */
export function prefetchRoute(path: string) {
  const p = path.split('?')[0];
  for (const [re, names] of ROUTE_PAGES) if (re.test(p)) names.forEach(load);
}

/** 로그인 후 브라우저가 한가할 때 자주 쓰는 화면을 미리 받는다 (데이터 절약 모드·느린 회선에서는 생략) */
export function prefetchCommonRoutes() {
  const conn = (navigator as Navigator & { connection?: { saveData?: boolean; effectiveType?: string } }).connection;
  if (conn?.saveData || /2g/.test(conn?.effectiveType ?? '')) return;
  // 위자드(가장 큰 청크)는 메뉴 호버 때만 받는다
  const run = () => (['Dashboard', 'Releases', 'ReleaseDetail'] as PageName[]).forEach(load);
  if ('requestIdleCallback' in window) window.requestIdleCallback(run, { timeout: 4000 });
  else setTimeout(run, 1500);
}

/** 앱 시작 즉시 현재 주소의 화면 청크를 받기 시작 — 인증 확인과 병렬로 진행돼 첫 화면이 빨라진다 */
export function prefetchInitialRoute() {
  const path = (window.location.hash.replace(/^#/, '') || '/').split('?')[0];
  if (/^\/(login|signup|find-account)/.test(path)) {
    load(path.startsWith('/signup') ? 'Signup' : path.startsWith('/find') ? 'FindAccount' : 'Login');
  } else {
    prefetchRoute(path);
  }
}
