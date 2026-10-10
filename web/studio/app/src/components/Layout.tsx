import { useEffect, useRef, useState, type ReactNode } from 'react';
import { Link, useLocation, useNavigate } from '../lib/router';
import { useAuth } from '../api/auth';
import { useProfile } from '../store/profile';
import { useUnreadCount } from '../store/support';
import { isPaymentRegistered, usePayment } from '../store/payment';
import { useDocs } from '../store/docs';
import { api } from '../api/client';
import { Badge } from '../ui';
import type { Tone } from '../ui/tone';
import { useConfirm } from './Confirm';
import { useToast } from './Toast';
import { LIVE_NOTICE_EVENT } from '../lib/liveEvents';
import { prefetchCommonRoutes, prefetchRoute } from '../routes';
import { useScrollReveal } from '../hooks/useScrollReveal';
import { Glyph, type GlyphName } from './Glyph';

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

type NavItem = { to: string; label: string; icon: GlyphName };

/** 전체 메뉴 — 계정 정보 화면처럼 옅은 파랑 묶음 카드에 아이콘 · 이름 · 꺾쇠 */
const NAV_GROUPS: { group: string; items: NavItem[] }[] = [
  {
    group: '내 작업실',
    items: [
      { to: '/', label: '홈', icon: 'home' },
      { to: '/releases', label: '발매·곡 관리', icon: 'disc' },
      { to: '/reports', label: '음악 리포트', icon: 'chart' },
      { to: '/settlement', label: '정산·지급', icon: 'won' },
    ],
  },
  {
    group: '계약·권리',
    items: [
      { to: '/contracts', label: '계약서', icon: 'sign' },
      { to: '/rights', label: '권리·보완 서류', icon: 'doc' },
    ],
  },
  {
    group: '계정·지원',
    items: [
      { to: '/profile', label: '계정 정보', icon: 'user' },
      { to: '/notifications', label: '알림', icon: 'bell' },
      { to: '/inquiries', label: '문의', icon: 'inquiry' },
      { to: '/events', label: '이벤트', icon: 'star' },
      { to: '/notices', label: '공지사항', icon: 'antenna' },
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
  const payment = usePayment();
  const docs = useDocs();
  // 메뉴를 열 때만 발매 목록을 한 번 불러와 '보완·작성 중' 개수를 메뉴에 보여 준다
  const [relCounts, setRelCounts] = useState<{ needs: number; drafts: number } | null>(null);
  const [menuOpen, setMenuOpen] = useState(false);
  useEffect(() => {
    if (!menuOpen) return;
    let alive = true;
    api.listReleases()
      .then(rs => { if (alive) setRelCounts({ needs: rs.filter(r => r.status === 'needs').length, drafts: rs.filter(r => r.status === 'draft').length }); })
      .catch(() => {});
    return () => { alive = false; };
  }, [menuOpen]);
  // 실시간으로 들어온 새 알림 — 화면을 보고 있으면 토스트로, 누르면 해당 화면으로
  useEffect(() => {
    const on = (e: Event) => {
      const n = (e as CustomEvent<{ title: string }>).detail;
      if (n?.title) toast(n.title, 'info');
    };
    window.addEventListener(LIVE_NOTICE_EVENT, on);
    return () => window.removeEventListener(LIVE_NOTICE_EVENT, on);
  }, [toast]);
  // 탭 제목에 읽지 않은 알림 수
  useEffect(() => {
    const base = document.title.replace(/^\(\d+\+?\) /, '');
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
  useScrollReveal(loc.pathname);
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
              <button type="button" className="aq-nav-me-alert" onClick={() => go('/profile')}>
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
            </div>
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
