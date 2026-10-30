import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { Link, savedScroll, useLocation, useNavigate, useNavigationPending } from '../lib/router';
import { useAuth } from '../api/auth';
import { useProfile } from '../store/profile';
import { useNotices, useUnreadCount } from '../store/support';
import { isPaymentRegistered, usePayment } from '../store/payment';
import { useDocs } from '../store/docs';
import { api } from '../api/client';
import { Badge } from '../ui';
import type { Tone } from '../ui/tone';
import { useConfirm } from './Confirm';
import { useToast } from './Toast';
import { LIVE_NOTICE_EVENT } from '../lib/liveEvents';
import { prefetchCommonRoutes, prefetchRoute } from '../routes';
import { useAsync, useCacheClears } from '../hooks/useAsync';
import { Glyph, type GlyphName } from './Glyph';
import '../styles/split/Layout.css';

// 라우트 변경 시 view 진입 애니메이션만 재시작 (children remount 없음 → useEffect/API 재실행 방지)
function ViewEnter({ pathname, children }: { pathname: string; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  const first = useRef(true);
  // 그리기 전에(useLayoutEffect) 다시 건다 — useEffect에서는 새 화면이 한 프레임 다 보였다가 사라진 뒤 들어와 깜빡였다
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (first.current) { first.current = false; return; } // 첫 마운트는 CSS가 자동 재생
    el.classList.remove('view-enter');
    // 스타일만 다시 계산해 애니메이션을 처음부터 — offsetWidth는 새 화면 전체의 배치까지 미리 계산하게 했다
    void getComputedStyle(el).animationName;
    el.classList.add('view-enter');
  }, [pathname]);
  return <div ref={ref} className="view-enter">{children}</div>;
}

/** 다음 화면을 준비하는 동안 머리 맨 위에 얇은 막대 — 지금 화면은 그대로 두고 '가고 있어요'만 알린다.
 *  금방 끝나는 이동(150ms 안)에는 보이지 않고, 끝나면 끝까지 채운 뒤 사라진다 */
function RouteBar() {
  const pending = useNavigationPending();
  const [phase, setPhase] = useState<'idle' | 'run' | 'done'>('idle');
  useEffect(() => {
    if (pending) setPhase('run');
    else setPhase(p => (p === 'run' ? 'done' : p));
  }, [pending]);
  useEffect(() => {
    if (phase !== 'done') return;
    const t = window.setTimeout(() => setPhase('idle'), 450);
    return () => window.clearTimeout(t);
  }, [phase]);
  return <span className={`aq-route-bar is-${phase}`} aria-hidden="true" />;
}

type NavItem = { to: string; label: string; icon: GlyphName };

/** 전체 메뉴 — 계정 정보 화면처럼 옅은 파랑 묶음 카드에 아이콘 · 이름 · 꺾쇠.
 *  하는 일 기준으로 묶고 자주 쓰는 것부터: 발매와 그 뒤에 이어지는 서명·보완을 한 묶음에 두어 할 일 배지가 한곳에 보이고,
 *  아티스트가 가장 자주 확인하는 돈과 성과를 바로 다음에 둔다. 알림·공지·이벤트는 '소식', 계정과 문의는 맨 아래 */
const NAV_GROUPS: { group: string; items: NavItem[] }[] = [
  {
    group: '작업실',
    items: [
      { to: '/', label: '홈', icon: 'home' },
      { to: '/releases', label: '발매·곡 관리', icon: 'disc' },
      { to: '/publishing', label: '퍼블리싱', icon: 'scale' },
      { to: '/contracts', label: '계약서', icon: 'sign' },
      { to: '/rights', label: '권리·보완 서류', icon: 'doc' },
    ],
  },
  {
    group: '수익·성과',
    items: [
      { to: '/settlement', label: '정산·지급', icon: 'won' },
      { to: '/reports', label: '음악 리포트', icon: 'chart' },
    ],
  },
  {
    group: '홍보',
    items: [
      { to: '/artist', label: '아티스트 페이지', icon: 'globe' },
    ],
  },
  {
    group: '소식',
    items: [
      { to: '/notifications', label: '알림', icon: 'bell' },
      { to: '/notices', label: '공지사항', icon: 'antenna' },
      { to: '/events', label: '이벤트', icon: 'star' },
    ],
  },
  {
    group: '계정·지원',
    items: [
      { to: '/profile', label: '계정 정보', icon: 'user' },
      { to: '/inquiries', label: '문의', icon: 'inquiry' },
    ],
  },
];

// 곡 상세·곡 추가는 퍼블리싱 안의 화면 — 메뉴는 '퍼블리싱'에 불이 들어온다
const MENU_ALIAS: [RegExp, string][] = [[/^\/works(\/|$)/, '/publishing']];
const isActive = (pathname: string, to: string) =>
  to === '/' ? pathname === '/' : pathname === to || pathname.startsWith(to + '/') || MENU_ALIAS.some(([re, t]) => t === to && re.test(pathname));

const APP_TITLE = 'AUDENIQ STUDIO';
/** 탭 제목에 쓸 화면 이름 — 메뉴 이름을 그대로 쓰고, 홈과 메뉴에 없는 화면은 앱 이름만 */
function pageLabel(pathname: string): string {
  if (pathname === '/upload') return '새 발매';
  if (pathname.startsWith('/publishing/apply')) return '저작권 관리 신청';
  for (const g of NAV_GROUPS) for (const item of g.items) if (item.to !== '/' && isActive(pathname, item.to)) return item.label;
  return '';
}

const NAV_EXIT_MS = 160;

export function Layout({ children }: { children: ReactNode }) {
  const loc = useLocation();
  const nav = useNavigate();
  const toast = useToast();
  const confirm = useConfirm();
  const { user, logout } = useAuth();
  const profile = useProfile();
  const unread = useUnreadCount();
  const payment = usePayment();
  const docs = useDocs();
  // 메뉴·왼쪽 메뉴의 '보완·작성 중' 개수 — 화면을 옮길 때마다 받지 않고, 바뀌었을 수 있을 때만 새로 받는다:
  // 무언가 바꾼 뒤(캐시를 비움), 새 알림이 왔을 때(담당자 처리), 다른 탭·앱에서 돌아왔을 때(15초에 한 번), 메뉴를 열 때.
  // 화면들과 같은 발매 목록 캐시('releases')를 쓴다 — 발매 목록·홈 화면이 받은 목록이 그대로 개수에 반영된다
  const [menuOpen, setMenuOpen] = useState(false);
  // 메뉴를 연 횟수 — 열 때만 새로 받는다 (menuOpen을 그대로 쓰면 닫을 때도 한 번 더 받았다)
  const [menuOpens, setMenuOpens] = useState(0);
  useEffect(() => { if (menuOpen) setMenuOpens(n => n + 1); }, [menuOpen]);
  const cacheClears = useCacheClears();
  const notices = useNotices();
  const latestNotice = `${notices.length}:${notices[0]?.id ?? ''}:${notices.at(-1)?.id ?? ''}`;
  const [returned, setReturned] = useState(0);
  useEffect(() => {
    let last = Date.now();
    const onVisible = () => {
      if (document.visibilityState !== 'visible' || Date.now() - last < 15_000) return;
      last = Date.now();
      setReturned(n => n + 1);
    };
    document.addEventListener('visibilitychange', onVisible);
    return () => document.removeEventListener('visibilitychange', onVisible);
  }, []);
  const { data: rels } = useAsync(() => api.listReleases(), [menuOpens, cacheClears, latestNotice, returned], { key: 'releases' });
  const relCounts = useMemo(() => (rels ? {
    needs: rels.filter(r => r.status === 'needs').length,
    drafts: rels.filter(r => r.status === 'draft').length,
  } : null), [rels]);
  // 실시간으로 들어온 새 알림 — 화면을 보고 있으면 토스트로, 누르면 해당 화면으로
  useEffect(() => {
    const on = (e: Event) => {
      const n = (e as CustomEvent<{ title: string }>).detail;
      if (n?.title) toast(n.title, 'info');
    };
    window.addEventListener(LIVE_NOTICE_EVENT, on);
    return () => window.removeEventListener(LIVE_NOTICE_EVENT, on);
  }, [toast]);
  // 탭 제목 — 지금 화면 이름 + 읽지 않은 알림 수. 모든 화면이 'AUDENIQ STUDIO'면 탭·방문 기록에서 구분이 안 되고
  // 화면이 바뀐 것을 스크린리더가 알려 주지 못한다
  useEffect(() => {
    const label = pageLabel(loc.pathname);
    const base = label ? `${label} · ${APP_TITLE}` : APP_TITLE;
    document.title = unread ? `(${unread > 99 ? '99+' : unread}) ${base}` : base;
  }, [unread, loc.pathname]);
  const [menuClosing, setMenuClosing] = useState(false);
  const [scrolled, setScrolled] = useState(false);
  const closeTimer = useRef<number | null>(null);

  const closeMenu = () => {
    if (!menuOpen) return;
    setMenuOpen(false);
    setMenuClosing(true);
    if (closeTimer.current) window.clearTimeout(closeTimer.current);
    closeTimer.current = window.setTimeout(() => setMenuClosing(false), NAV_EXIT_MS);
  };
  const closeMenuRef = useRef(closeMenu);
  closeMenuRef.current = closeMenu;

  useEffect(() => () => { if (closeTimer.current) window.clearTimeout(closeTimer.current); }, []);

  // 로그인 직후 한가한 시간에 주요 화면 청크를 미리 받아 첫 이동을 빠르게
  useEffect(() => { prefetchCommonRoutes(); }, []);

  useEffect(() => {
    let ticking = false;
    const onScroll = () => {
      if (ticking) return;
      ticking = true;
      requestAnimationFrame(() => {
        setScrolled(window.scrollY > 8);
        ticking = false;
      });
    };
    window.addEventListener('scroll', onScroll, { passive: true });
    onScroll();
    return () => window.removeEventListener('scroll', onScroll);
  }, []);

  // 헤더 바깥 클릭·Esc로 메뉴 닫기
  useEffect(() => {
    if (!menuOpen) return;
    const onClick = (e: MouseEvent) => {
      if (!(e.target as HTMLElement).closest('.portal-header')) closeMenuRef.current();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        closeMenuRef.current();
        document.getElementById('menuToggle')?.focus();
      }
    };
    // 태블릿을 가로로 돌려 왼쪽 고정 메뉴가 나오는 폭(1024px)이 되면 닫는다 — 닫기 버튼이 사라져 열린 채로 남았다
    const wide = window.matchMedia?.('(min-width: 1024px)');
    const onWide = (e: MediaQueryListEvent) => { if (e.matches) closeMenuRef.current(); };
    wide?.addEventListener('change', onWide);
    document.addEventListener('click', onClick);
    document.addEventListener('keydown', onKey);
    return () => {
      wide?.removeEventListener('change', onWide);
      document.removeEventListener('click', onClick);
      document.removeEventListener('keydown', onKey);
    };
  }, [menuOpen]);

  // 화면이 바뀌면 메뉴를 닫고, 새 화면은 맨 위에서 · 뒤로 가기는 보던 자리에서 연다.
  // html의 부드러운 스크롤을 타면 새 화면이 그려지는 도중에 멈춰 중간에서 열리므로 즉시 이동한다.
  const lastPath = useRef(loc.pathname);
  useLayoutEffect(() => {
    setMenuOpen(false);
    // 같은 화면에서 탭·필터만 바꾼 주소(REPLACE)는 보던 자리를 지킨다
    const samePage = lastPath.current === loc.pathname;
    lastPath.current = loc.pathname;
    if (samePage && loc.action !== 'PUSH' && loc.action !== 'POP') return;
    const target = loc.action === 'POP' ? savedScroll(loc.key) : 0;
    const jump = (y: number) => window.scrollTo({ top: y, left: 0, behavior: 'instant' as ScrollBehavior });
    jump(target);
    if (!target) return;
    // 돌아간 화면의 목록이 아직 불러와지는 중이면 높이가 생길 때까지 몇 번 더 맞춘다
    let tries = 0;
    let raf = 0;
    const again = () => {
      if (Math.abs(window.scrollY - target) < 2 || ++tries > 30) return;
      jump(target);
      raf = requestAnimationFrame(again);
    };
    raf = requestAnimationFrame(again);
    const stop = () => cancelAnimationFrame(raf);
    window.addEventListener('wheel', stop, { once: true, passive: true });
    window.addEventListener('touchstart', stop, { once: true, passive: true });
    return () => { stop(); window.removeEventListener('wheel', stop); window.removeEventListener('touchstart', stop); };
  }, [loc.pathname, loc.key]); // eslint-disable-line react-hooks/exhaustive-deps

  // 발매 신청 화면은 머리글 없는 전용 레이아웃 — 화면 청크(가장 큼)를 받는 동안의 뼈대부터 같은 레이아웃으로 그려
  // 다 받은 뒤 머리글이 사라지며 화면 전체가 위로 튀지 않게 한다 (Upload도 마운트될 때 같은 클래스를 단다)
  useLayoutEffect(() => {
    document.body.classList.toggle('wizard-mode', loc.pathname === '/upload');
  }, [loc.pathname]);

  // 메뉴는 열 때마다 맨 위부터 (지난번에 내려 둔 자리에서 열리지 않게), 열려 있는 동안 뒤 화면은 멈춘다
  useLayoutEffect(() => {
    if (menuOpen) document.getElementById('portalNav')?.scrollTo({ top: 0, behavior: 'instant' as ScrollBehavior });
    document.documentElement.classList.toggle('aq-menu-open', menuOpen);
    return () => document.documentElement.classList.remove('aq-menu-open');
  }, [menuOpen]);

  const go = (to: string) => {
    closeMenu();
    if (to !== loc.pathname) nav(to);
  };

  const doLogout = async () => {
    closeMenu();
    const ok = await confirm({ title: '로그아웃할까요?', message: '작성 중인 발매는 임시 저장돼 있어요.', confirmLabel: '로그아웃', danger: true });
    if (!ok) return;
    await logout();
    toast('로그아웃했어요.', 'info');
    nav('/login', { replace: true });
  };

  // 메뉴마다 지금 처리할 일 — 대시보드 '확인할 작업'과 같은 기준, AUDENIQ 상태 색(보완=주황, 서명=파랑, 작성 중=회색)
  const toSign = docs.filter(d => d.kind === 'agreements' && d.reviewStatus === 'approved' && !d.localSignatureAt).length;
  const docFix = docs.filter(d => d.kind === 'rights' && d.reviewStatus === 'needs').length;
  const docNeeded = docs.filter(d => d.kind === 'rights' && !['review', 'prepared', 'approved'].includes(d.reviewStatus)).length;
  const navStatus: Record<string, { tone: Tone; text: string } | undefined> = {
    '/releases': relCounts?.needs ? { tone: 'amber', text: `보완 ${relCounts.needs}` }
      : relCounts?.drafts ? { tone: 'gray', text: `작성 중 ${relCounts.drafts}` } : undefined,
    '/contracts': toSign ? { tone: 'blue', text: `서명 ${toSign}` } : undefined,
    '/rights': docFix ? { tone: 'amber', text: `보완 ${docFix}` } : docNeeded ? { tone: 'blue', text: `제출 ${docNeeded}` } : undefined,
  };
  const initial = (profile.name || user?.email || 'A').slice(0, 1).toUpperCase();
  const navVisible = menuOpen || menuClosing;

  return (
    <>
      <a className="aq-skip-link" href="#main" onClick={e => { e.preventDefault(); document.getElementById('main')?.focus(); }}>
        본문으로 건너뛰기
      </a>
      <header className={`portal-header${scrolled ? ' is-scrolled' : ''}`}>
        <RouteBar />
        <div className="nav-shell">
          <Link to="/" className="brand" aria-label="AUDENIQ STUDIO 홈">
            <img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" />
            <span className="workspace-label ui-product">STUDIO</span>
          </Link>
          <div className="header-right">
            <button
              type="button"
              className="aq-header-bell"
              aria-label={unread ? `알림 ${unread}건 읽지 않음` : '알림'}
              onClick={() => go('/notifications')}
            >
              <Glyph name="bell" size={20} />
              {unread > 0 && <span className="aq-badge" key={unread}>{unread > 9 ? '9+' : unread}</span>}
            </button>
            <button
              id="headerProfile"
              className="ui-avatar header-profile"
              type="button"
              aria-label="계정 정보"
              onClick={() => go('/profile')}
            >
              {initial}
            </button>
            <button
              type="button"
              id="menuToggle"
              className="menu-toggle"
              aria-label={menuOpen ? '메뉴 닫기' : '메뉴 열기'}
              aria-controls="portalNav"
              aria-expanded={menuOpen}
              onClick={() => (menuOpen ? closeMenu() : setMenuOpen(true))}
            >
              <span></span>
              <span></span>
            </button>
          </div>
        </div>
        <nav
          className={`portal-nav${menuClosing && !menuOpen ? ' is-closing' : ''}`}
          id="portalNav"
          aria-label="아티스트 포털 전체 메뉴"
          hidden={!navVisible}
        >
          {/* 내 계정 — 계정 정보 페이지와 같은 아바타, 정산 계좌가 없으면 알림 줄, 가장 자주 하는 '새 발매'는 큰 버튼 */}
          <div className={`aq-nav-me${isActive(loc.pathname, '/profile') ? ' is-active' : ''}`}>
            <button type="button" className="aq-nav-me-row" onClick={() => go('/profile')} onPointerEnter={() => prefetchRoute('/profile')}>
              <i className="ui-avatar aq-profile-avatar aq-nav-me-avatar" aria-hidden="true">{initial}</i>
              <i className="aq-nav-me-text">
                <strong>{profile.name || '활동명을 입력해 주세요'}</strong>
                <small>{user?.email ?? 'AUDENIQ STUDIO'}</small>
              </i>
              <Glyph name="chevron-right" size={18} className="aq-nav-me-chev" />
            </button>
            {!isPaymentRegistered(payment) && (
              <button type="button" className="aq-nav-me-alert" onClick={() => go('/profile?open=payout')}>
                <Glyph name="won" size={15} />
                <i>수익을 받을 정산 계좌를 등록해 주세요</i>
                <Glyph name="chevron-right" size={14} />
              </button>
            )}
            <button
              type="button" className={`aq-nav-cta${isActive(loc.pathname, '/upload') ? ' is-current' : ''}`}
              onClick={() => go('/upload')} onPointerEnter={() => prefetchRoute('/upload')}
            >
              <Glyph name="plus" size={17} /> 새 발매 등록하기
            </button>
          </div>
          {NAV_GROUPS.map(g => (
            <div key={g.group} className="aq-nav-card">
              <div className="nav-group">{g.group}</div>
              {g.items.map(item => {
                const active = isActive(loc.pathname, item.to);
                return (
                  <button
                    key={item.to}
                    type="button"
                    className={active ? 'active' : ''}
                    aria-current={active ? 'page' : undefined}
                    onClick={() => go(item.to)}
                    onPointerEnter={() => prefetchRoute(item.to)}
                    onFocus={() => prefetchRoute(item.to)}
                  >
                    <i className="aq-nav-ico" aria-hidden="true"><Glyph name={item.icon} size={18} /></i>
                    <em className="aq-nav-label">
                      {item.label}
                      {item.to === '/notifications' && unread > 0 && <b className="aq-nav-count">{unread}</b>}
                    </em>
                    <span className="aq-nav-end">
                      {navStatus[item.to] && <Badge tone={navStatus[item.to]!.tone} className="aq-nav-status">{navStatus[item.to]!.text}</Badge>}
                      <Glyph name="chevron-right" size={16} />
                    </span>
                  </button>
                );
              })}
            </div>
          ))}
          <div className="nav-foot aq-nav-foot">
            <div className="aq-nav-legal">
              <button type="button" onClick={() => go('/terms')}>이용약관</button>
              <button type="button" onClick={() => go('/terms/settlement')}>정산 약관</button>
              <button type="button" onClick={() => go('/privacy')}>개인정보 처리방침</button>
              <button type="button" onClick={() => go('/rights-report')}>권리침해 신고</button>
            </div>
            <button type="button" className="aq-logout" onClick={doLogout}>로그아웃</button>
          </div>
        </nav>
      </header>

      {/* 데스크톱 왼쪽 고정 메뉴 — 넓은 화면에서는 메뉴를 숨기지 않고 늘 보여 준다 (숨긴 메뉴는 덜 발견되고 이동이 느리다)
          화면에서 본문보다 먼저 보이므로 Tab 순서도 본문 앞에 둔다 (고정 위치라 배치는 같다, '본문으로 건너뛰기'가 이 메뉴를 건너뛴다) */}
      <nav className="aq-sidebar" aria-label="전체 메뉴">
        {/* 가장 자주 하는 일은 메뉴 맨 위에 — 휴대폰 전체 메뉴의 '새 발매 등록하기'와 같은 자리 */}
        <Link to="/upload" className="ui-btn is-soft aq-side-cta" onPointerEnter={() => prefetchRoute('/upload')}>
          <Glyph name="plus" size={16} />새 발매
        </Link>
        {NAV_GROUPS.map(g => (
          <div key={g.group} className="aq-side-group">
            <p className="aq-side-title">{g.group}</p>
            {g.items.map(item => {
              const active = isActive(loc.pathname, item.to);
              return (
                <Link key={item.to} to={item.to} className={`aq-side-item${active ? ' is-active' : ''}`} aria-current={active ? 'page' : undefined}
                  onPointerEnter={() => prefetchRoute(item.to)}>
                  <Glyph name={item.icon} size={18} />
                  <span>{item.label}</span>
                  {item.to === '/notifications' && unread > 0 && <span className="aq-side-badge">{unread > 99 ? '99+' : unread}</span>}
                  {navStatus[item.to] && <Badge tone={navStatus[item.to]!.tone} className="aq-side-status">{navStatus[item.to]!.text}</Badge>}
                </Link>
              );
            })}
          </div>
        ))}
      </nav>

      <main className="portal-layout" id="main" tabIndex={-1}>
        <ViewEnter pathname={loc.pathname}>
          {children}
        </ViewEnter>
      </main>
    </>
  );
}

