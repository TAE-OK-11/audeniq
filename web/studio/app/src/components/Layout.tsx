import { useEffect, useRef, useState, type ReactNode } from 'react';
import { Link, useLocation, useNavigate } from '../lib/router';
import { useAuth } from '../api/auth';
import { useProfile } from '../store/profile';
import { useUnreadCount } from '../store/support';
import { useConfirm } from './Confirm';
import { useToast } from './Toast';
import { prefetchCommonRoutes, prefetchRoute } from '../routes';

// 라우트 변경 시 view 진입 애니메이션만 재시작 (children remount 없음 → useEffect/API 재실행 방지)
function ViewEnter({ pathname, children }: { pathname: string; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  const first = useRef(true);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (first.current) { first.current = false; return; } // 첫 마운트는 CSS가 자동 재생
    el.classList.remove('view-enter');
    void el.offsetWidth; // reflow로 애니메이션 리트리거
    el.classList.add('view-enter');
  }, [pathname]);
  return <div ref={ref} className="view-enter">{children}</div>;
}

const NAV_GROUPS: { group: string; items: { to: string; label: string }[] }[] = [
  {
    group: '내 작업실',
    items: [
      { to: '/', label: '홈' },
      { to: '/upload', label: '새로운 발매' },
      { to: '/releases', label: '발매·곡 관리' },
      { to: '/reports', label: '음악 리포트' },
      { to: '/settlement', label: '정산·지급' },
    ],
  },
  {
    group: '계약·권리',
    items: [
      { to: '/contracts', label: 'AUDENIQ 계약서' },
      { to: '/rights', label: '권리·보완 서류' },
    ],
  },
  {
    group: '계정·지원',
    items: [
      { to: '/inquiries', label: '문의' },
      { to: '/notifications', label: '알림' },
      { to: '/events', label: '이벤트' },
      { to: '/notices', label: '공지사항' },
      { to: '/profile', label: '아티스트 정보' },
    ],
  },
];

const isActive = (pathname: string, to: string) =>
  to === '/' ? pathname === '/' : pathname === to || pathname.startsWith(to + '/');

const NAV_EXIT_MS = 160;

export function Layout({ children }: { children: ReactNode }) {
  const loc = useLocation();
  const nav = useNavigate();
  const toast = useToast();
  const confirm = useConfirm();
  const { user, logout } = useAuth();
  const profile = useProfile();
  const unread = useUnreadCount();
  const [menuOpen, setMenuOpen] = useState(false);
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
    document.addEventListener('click', onClick);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('click', onClick);
      document.removeEventListener('keydown', onKey);
    };
  }, [menuOpen]);

  // 페이지 전환 시 메뉴 닫고 맨 위로 스크롤
  useEffect(() => {
    setMenuOpen(false);
    window.scrollTo(0, 0);
  }, [loc.pathname]);

  const go = (to: string) => {
    closeMenu();
    if (to !== loc.pathname) nav(to);
  };

  const doLogout = async () => {
    closeMenu();
    const ok = await confirm({ title: '로그아웃할까요?', message: '작성 중인 발매는 임시 저장돼 있어요.', confirmLabel: '로그아웃' });
    if (!ok) return;
    await logout();
    toast('로그아웃했어요.', 'info');
    nav('/login', { replace: true });
  };

  const initial = (profile.name || user?.email || 'A').slice(0, 1).toUpperCase();
  const navVisible = menuOpen || menuClosing;

  return (
    <>
      <a className="aq-skip-link" href="#main" onClick={e => { e.preventDefault(); document.getElementById('main')?.focus(); }}>
        본문으로 건너뛰기
      </a>
      <header className={`portal-header${scrolled ? ' is-scrolled' : ''}`}>
        <div className="nav-shell">
          <Link to="/" className="brand" aria-label="AUDENIQ STUDIO 홈">
            <img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" />
          </Link>
          <div className="header-right">
            <span className="workspace-label">STUDIO</span>
            <button
              type="button"
              className="aq-header-bell"
              aria-label={unread ? `알림 ${unread}건 읽지 않음` : '알림'}
              onClick={() => go('/notifications')}
            >
              <svg viewBox="0 0 24 24" width="20" height="20" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round">
                <path d="M6 8a6 6 0 1 1 12 0c0 7 3 9 3 9H3s3-2 3-9" /><path d="M10.3 21a1.94 1.94 0 0 0 3.4 0" />
              </svg>
              {unread > 0 && <span className="aq-badge" key={unread}>{unread > 9 ? '9+' : unread}</span>}
            </button>
            <button
              id="headerProfile"
              className="header-profile"
              type="button"
              aria-label="아티스트 정보"
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
          {NAV_GROUPS.map(g => (
            <div key={g.group}>
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
                    <em className="aq-nav-label">
                      {item.label}
                      {item.to === '/notifications' && unread > 0 && <b className="aq-nav-count">{unread}</b>}
                    </em>
                    <span>↗</span>
                  </button>
                );
              })}
            </div>
          ))}
          <div className="nav-foot aq-nav-foot">
            <span className="aq-nav-account">{user?.email ?? 'AUDENIQ / STUDIO'}</span>
            <button type="button" className="aq-logout" onClick={doLogout}>로그아웃</button>
          </div>
        </nav>
      </header>

      <main className="portal-layout" id="main" tabIndex={-1}>
        <ViewEnter pathname={loc.pathname}>
          {children}
        </ViewEnter>
      </main>
    </>
  );
}
