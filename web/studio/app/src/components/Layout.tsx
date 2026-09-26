import { useEffect, useRef, useState } from 'react';
import { Link, useLocation, useNavigate } from 'react-router-dom';
import { useAuth } from '../api/auth';
import type { ReactNode } from 'react';

// 라이브 HTML의 portalNav와 동일한 구조
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

  // 라이브와 동일: 헤더 바깥 클릭 시 메뉴 닫기
  // iOS 스크롤 체이닝 방지: 메뉴 열림 시 body 스크롤 잠금
  useEffect(() => {
    if (!menuOpen) return;
    const scrollY = window.scrollY;
    const onClick = (e: MouseEvent) => {
      if (!(e.target as HTMLElement).closest('.portal-header')) {
        setMenuOpen(false);
      }
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setMenuOpen(false);
    };
    document.addEventListener('click', onClick);
    document.addEventListener('keydown', onKey);
    // body 스크롤 잠금 (위치 유지)
    document.body.style.position = 'fixed';
    document.body.style.top = `-${scrollY}px`;
    document.body.style.left = '0';
    document.body.style.right = '0';
    return () => {
      document.removeEventListener('click', onClick);
      document.removeEventListener('keydown', onKey);
      document.body.style.position = '';
      document.body.style.top = '';
      document.body.style.left = '';
      document.body.style.right = '';
      window.scrollTo(0, scrollY);
    };
  }, [menuOpen]);

  useEffect(() => setMenuOpen(false), [loc.pathname]);

  // 페이지 전환 시 맨 위로 스크롤 (스크롤 위치 유지 버그 수정)
  useEffect(() => {
    window.scrollTo(0, 0);
  }, [loc.pathname]);

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
              id="headerProfile"
              className="header-profile"
              type="button"
              aria-label="아티스트 계정"
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
              onClick={() => setMenuOpen(v => !v)}
            >
              <span></span>
              <span></span>
            </button>
          </div>
        </div>
        <nav
          className="portal-nav"
          id="portalNav"
          aria-label="아티스트 포털 전체 메뉴"
          hidden={!menuOpen}
        >
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

      <main className="portal-layout" id="main">
        {/* 라우트 변경 시에만 view 진입 애니메이션 재생 (리렌더에는 재생 안 됨) */}
        <div key={loc.pathname} className="view-enter">
          {children}
        </div>
      </main>
    </>
  );
}
