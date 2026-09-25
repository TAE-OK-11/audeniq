import { useEffect, useRef, useState } from 'react';
import { Link, useLocation, useNavigate } from 'react-router-dom';
import { useAuth } from '../api/auth';
import type { ReactNode } from 'react';

// Exact nav structure from studio HTML
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
      { to: '/support', label: '문의·알림' },
      { to: '/profile', label: '아티스트·정산 정보' },
    ],
  },
];

export function Layout({ children }: { children: ReactNode }) {
  const loc = useLocation();
  const nav = useNavigate();
  const { user } = useAuth();
  const [menuOpen, setMenuOpen] = useState(false);
  const [scrolled, setScrolled] = useState(false);
  const headerRef = useRef<HTMLElement>(null);

  // Scroll shadow — exact: toggle at scrollY > 8
  useEffect(() => {
    const onScroll = () => setScrolled(window.scrollY > 8);
    window.addEventListener('scroll', onScroll, { passive: true });
    onScroll();
    return () => window.removeEventListener('scroll', onScroll);
  }, []);

  // Click outside closes nav — exact HTML behavior
  useEffect(() => {
    if (!menuOpen) return;
    const onDocClick = (e: MouseEvent) => {
      if (!headerRef.current?.contains(e.target as Node)) setMenuOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setMenuOpen(false);
    };
    document.addEventListener('click', onDocClick);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('click', onDocClick);
      document.removeEventListener('keydown', onKey);
    };
  }, [menuOpen]);

  // Close nav on route change
  useEffect(() => setMenuOpen(false), [loc.pathname]);

  const go = (to: string) => {
    setMenuOpen(false);
    nav(to);
  };

  const initial = (user?.email?.[0] ?? 'A').toUpperCase();

  return (
    <>
      <header ref={headerRef} className={`portal-header${scrolled ? ' is-scrolled' : ''}`}>
        <div className="nav-shell">
          <Link to="/" className="brand" aria-label="AUDENIQ STUDIO 홈">
            <img src="/connected/assets/AUDENIQ_Logo_Light.svg" alt="AUDENIQ" />
          </Link>
          <div className="header-right">
            <span className="workspace-label">STUDIO</span>
            <button
              className="header-profile"
              type="button"
              aria-label="아티스트 계정"
              onClick={() => go('/profile')}
            >
              {initial}
            </button>
            <button
              type="button"
              className="menu-toggle"
              aria-label={menuOpen ? '메뉴 닫기' : '메뉴 열기'}
              aria-controls="portalNav"
              aria-expanded={menuOpen}
              onClick={() => setMenuOpen(v => !v)}
            >
              <span></span>
              <span></span>
            </button>
          </div>
        </div>
        <nav className="portal-nav" id="portalNav" aria-label="아티스트 포털 전체 메뉴" hidden={!menuOpen}>
          {NAV_GROUPS.map(g => (
            <div key={g.group}>
              <div className="nav-group">{g.group}</div>
              {g.items.map(item => (
                <button
                  key={item.to}
                  type="button"
                  className={loc.pathname === item.to ? 'active' : ''}
                  onClick={() => go(item.to)}
                >
                  {item.label} <span>↗</span>
                </button>
              ))}
            </div>
          ))}
          <div className="nav-foot">AUDENIQ / STUDIO</div>
        </nav>
      </header>
      <main className="portal-layout">{children}</main>
    </>
  );
}
