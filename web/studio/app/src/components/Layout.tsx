import { useEffect, useRef, useState } from 'react';
import { Link, useLocation, useNavigate } from 'react-router-dom';
import { useAuth } from '../api/auth';
import type { ReactNode } from 'react';

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

  useEffect(() => {
    const onScroll = () => setScrolled(window.scrollY > 8);
    window.addEventListener('scroll', onScroll, { passive: true });
    onScroll();
    return () => window.removeEventListener('scroll', onScroll);
  }, []);

  useEffect(() => {
    if (!menuOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setMenuOpen(false);
    };
    document.addEventListener('keydown', onKey);
    document.body.style.overflow = 'hidden';
    return () => {
      document.removeEventListener('keydown', onKey);
      document.body.style.overflow = '';
    };
  }, [menuOpen]);

  useEffect(() => setMenuOpen(false), [loc.pathname]);

  const go = (to: string) => {
    setMenuOpen(false);
    nav(to);
  };

  const initial = (user?.email?.[0] ?? '서').toUpperCase();

  return (
    <>
      <header ref={headerRef} className={`portal-header${scrolled ? ' is-scrolled' : ''}`}>
        <div className="nav-shell">
          <Link to="/" className="brand" aria-label="AUDENIQ STUDIO 홈">
            <img src="/connected/assets/AUDENIQ_Logo_Light.svg" alt="AUDENIQ" />
          </Link>
          <div className="header-right">
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
              aria-expanded={menuOpen}
              onClick={() => setMenuOpen(v => !v)}
            >
              {menuOpen ? (
                <svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg>
              ) : (
                <><span></span><span></span></>
              )}
            </button>
          </div>
        </div>
      </header>

      {menuOpen && (
        <div className="menu-overlay" role="dialog" aria-label="전체 메뉴">
          <div className="menu-sheet">
            <h2 className="menu-title">메뉴</h2>
            {NAV_GROUPS.map(g => (
              <div key={g.group} className="menu-group">
                <div className="menu-group-label">{g.group}</div>
                {g.items.map(item => (
                  <button
                    key={item.to}
                    type="button"
                    className={`menu-item${loc.pathname === item.to ? ' active' : ''}`}
                    onClick={() => go(item.to)}
                  >
                    {item.label}
                    <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round"><path d="m9 6 6 6-6 6"/></svg>
                  </button>
                ))}
              </div>
            ))}
          </div>
        </div>
      )}

      <main className="portal-layout">{children}</main>
    </>
  );
}
