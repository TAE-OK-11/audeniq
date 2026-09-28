// AUDENIQ ADMIN — 스태프 전용 콘솔. 스튜디오와 같은 계정으로 로그인하고,
// 스태프 역할(ADMIN·REVIEWER·OPERATOR·SUPPORT)은 서버 `/api/staff/me`가 알려준다.
import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';
import { Link, Navigate, Route, Routes, useLocation, useNavigate } from './lib/router';
import { useAuth } from './auth';
import { ApiError } from './api/errors';
import { MOCK } from './lib/mode';
import { useConfirm } from './components/Confirm';
import { useToast } from './components/Toast';
import { staffApi, type Duty, type Overview, type StaffMe } from './api/staff';
import { ROLE_LABEL } from './labels';
import { StaffContext } from './ui';
import { OverviewPage } from './pages/Overview';
import { ReviewQueue } from './pages/ReviewQueue';
import { ReviewDetail } from './pages/ReviewDetail';
import { Approvals } from './pages/Approvals';
import { Documents } from './pages/Documents';
import { Inquiries } from './pages/Inquiries';
import { Deliveries } from './pages/Deliveries';
import { Dsps } from './pages/Dsps';
import { Payouts } from './pages/Payouts';
import './styles/admin.css';
import { Glyph } from './components/Glyph';
import { setDspNames } from './lib/dspNames';

const STUDIO_URL = import.meta.env.VITE_STUDIO_URL ?? 'https://studio.audeniq.com';

// 관리자 메뉴 아이콘 — 스튜디오와 같은 AUDENIQ 아이콘 세트(Glyph)
const ICONS: Record<string, string> = {
  home: 'home', review: 'review', approval: 'approval', doc: 'doc', inquiry: 'inquiry',
  delivery: 'truck', dsp: 'globe', payout: 'card',
};

export function Icon({ name }: { name: string }) {
  return <Glyph name={ICONS[name] ?? name} size={18} />;
}

interface NavItem { to: string; label: string; icon: string; count?: (o: Overview) => number; hot?: boolean; adminOnly?: boolean; also?: string[] }
const NAV: { group: string; items: NavItem[] }[] = [
  { group: 'OVERVIEW', items: [{ to: '/', label: '오늘의 업무', icon: 'home' }] },
  {
    group: 'REVIEW',
    items: [
      // 2차 승인은 발매 심사 안의 탭
      { to: '/reviews', label: '발매 심사', icon: 'review', count: o => o.review + o.second_approvals, hot: true, also: ['/approvals'] },
      { to: '/documents', label: '서류 검토', icon: 'doc', count: o => o.documents },
    ],
  },
  { group: 'SUPPORT', items: [{ to: '/inquiries', label: '문의 답변', icon: 'inquiry', count: o => o.inquiries, hot: true }] },
  {
    group: 'DISTRIBUTION',
    items: [
      // 배급은 최종 승인 + 아티스트 서명 후 자동 — 여기서는 현황과 문제만 본다 (플랫폼 조건은 탭)
      { to: '/deliveries', label: '배급 현황', icon: 'delivery', count: o => o.deliveries_blocked, hot: true, also: ['/dsps'] },
      { to: '/payouts', label: '지급 요청', icon: 'payout', count: o => o.payout_requests, adminOnly: true },
    ],
  },
];

const active = (path: string, to: string) => (to === '/' ? path === '/' : path === to || path.startsWith(`${to}/`));

function Gate({ title, children, action }: { title: string; children: ReactNode; action?: ReactNode }) {
  return (
    <div className="adm-gate">
      <div className="adm-empty-icon" aria-hidden="true"><Glyph name="lock" size={26} /></div>
      <h1>{title}</h1>
      <p>{children}</p>
      {action}
    </div>
  );
}

export function AdminApp() {
  const loc = useLocation();
  const nav = useNavigate();
  const toast = useToast();
  const confirm = useConfirm();
  const { user, logout } = useAuth();
  const [me, setMe] = useState<StaffMe | null>(null);
  const [gate, setGate] = useState<'loading' | 'ok' | 'forbidden' | 'error'>('loading');
  const [counts, setCounts] = useState<Overview | null>(null);

  const loadMe = useCallback(() => {
    setGate('loading');
    staffApi.me()
      .then(m => { setMe(m); setGate('ok'); })
      .catch(e => setGate(e instanceof ApiError && e.status === 403 ? 'forbidden' : 'error'));
  }, []);
  useEffect(loadMe, [loadMe]);

  useEffect(() => {
    if (gate !== 'ok') return;
    let active = true;
    void staffApi.dsps().then(r => {
      if (active) setDspNames(r.items.map(d => ({ code: d.code, slug: d.slug, name: d.name_ko || d.name })));
    }).catch(() => {});
    return () => { active = false; setDspNames([]); };
  }, [gate]);

  const refreshCounts = useCallback(() => {
    staffApi.overview().then(setCounts).catch(() => { /* 숫자는 보조 정보 — 실패해도 화면은 쓸 수 있다 */ });
  }, []);
  useEffect(() => {
    if (gate !== 'ok') return;
    refreshCounts();
    // 여러 담당자가 동시에 처리하므로 1분마다 대기 건수를 새로 받는다 (탭이 보일 때만)
    const t = window.setInterval(() => { if (document.visibilityState === 'visible') refreshCounts(); }, 60_000);
    return () => window.clearInterval(t);
  }, [gate, refreshCounts]);

  useEffect(() => { window.scrollTo(0, 0); }, [loc.pathname]);
  useEffect(() => {
    const prev = document.title;
    document.title = 'AUDENIQ ADMIN';
    return () => { document.title = prev; };
  }, []);

  const ctx = useMemo(() => me && ({
    me, counts, refreshCounts, can: (d: Duty) => me.duties.includes(d),
  }), [me, counts, refreshCounts]);

  const doLogout = async () => {
    const ok = await confirm({ title: '로그아웃할까요?', confirmLabel: '로그아웃' });
    if (!ok) return;
    await logout();
    toast('로그아웃했어요.', 'info');
    nav('/login', { replace: true });
  };

  if (gate === 'loading') {
    return <div className="aq-boot" role="status" aria-label="관리자 화면을 불러오는 중"><img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="" /><span className="aq-boot-bar" /></div>;
  }
  if (gate === 'forbidden') {
    return (
      <Gate title="관리자 권한이 필요해요" action={<button type="button" className="adm-btn primary" onClick={() => { void logout().then(() => nav('/login', { replace: true })); }}>다른 계정으로 로그인</button>}>
        {user?.email ?? '이 계정'}은 AUDENIQ 스태프로 등록돼 있지 않아요. 권한은 운영 도구(<span className="adm-code">audeniq-admin staff grant</span>)로만 부여돼요.
      </Gate>
    );
  }
  if (gate === 'error' || !ctx || !me) {
    return (
      <Gate title="스태프 정보를 불러오지 못했어요" action={<button type="button" className="adm-btn primary" onClick={loadMe}>다시 시도</button>}>
        서버 연결을 확인한 뒤 다시 시도해 주세요.
      </Gate>
    );
  }

  const initial = (user?.email || 'A').slice(0, 1).toUpperCase();

  return (
    <StaffContext.Provider value={ctx}>
      <div className="adm-body">
        <a className="aq-skip-link" href="#main" onClick={e => { e.preventDefault(); document.getElementById('main')?.focus(); }}>본문으로 건너뛰기</a>
        <header className="adm-header">
          <div className="adm-header-shell">
            <Link to="/" className="adm-brand" aria-label="AUDENIQ ADMIN 홈">
              <img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" />
              <span className="adm-pill">ADMIN</span>
            </Link>
            <div className="adm-header-right">
              {MOCK && <span className="adm-chip adm-t-amber">체험 데이터</span>}
              <div className="adm-who" title={user?.email}>
                <span className="adm-who-avatar" aria-hidden="true">{initial}</span>
                <span className="adm-who-text"><b>{user?.email}</b><small>{ROLE_LABEL[me.role]}</small></span>
              </div>
              <a href={STUDIO_URL} className="adm-ghost" target="_blank" rel="noopener noreferrer" aria-label="스튜디오 새 창으로 열기"><Glyph name="arrow-up-right" size={15} /><span className="adm-ghost-text">스튜디오</span></a>
              <button type="button" className="adm-ghost" onClick={doLogout}>로그아웃</button>
            </div>
          </div>
        </header>

        <div className="adm-shell">
          <nav className="adm-side" aria-label="관리자 메뉴">
            {NAV.map(g => (
              <div className="adm-side-group" key={g.group}>
                <div className="adm-side-label">{g.group}</div>
                {g.items.filter(i => !i.adminOnly || me.role === 'ADMIN').map(i => {
                  const n = counts && i.count ? i.count(counts) : 0;
                  const cur = active(loc.pathname, i.to) || !!i.also?.some(a => active(loc.pathname, a));
                  return (
                    <Link key={i.to} to={i.to} aria-current={cur ? 'page' : undefined}>
                      <span><Icon name={i.icon} />{i.label}</span>
                      {n > 0 && <b className={`adm-count${i.hot ? ' is-hot' : ''}`}>{n > 99 ? '99+' : n}</b>}
                    </Link>
                  );
                })}
              </div>
            ))}
            <div className="adm-side-foot">
              {ROLE_LABEL[me.role]} 권한<br />모든 처리는 담당자 계정으로 감사 기록에 남아요.
            </div>
          </nav>

          <main className="adm-main" id="main" tabIndex={-1}>
            <Routes>
              <Route path="/" element={<OverviewPage />} />
              <Route path="/reviews" element={<ReviewQueue />} />
              <Route path="/reviews/:id" element={<ReviewDetail />} />
              <Route path="/approvals" element={<Approvals />} />
              <Route path="/documents" element={<Documents />} />
              <Route path="/inquiries" element={<Inquiries />} />
              <Route path="/inquiries/:id" element={<Inquiries />} />
              <Route path="/deliveries" element={<Deliveries />} />
              <Route path="/dsps" element={<Dsps />} />
              <Route path="/payouts" element={me.role === 'ADMIN' ? <Payouts /> : <Navigate to="/" replace />} />
              <Route path="*" element={<Navigate to="/" replace />} />
            </Routes>
          </main>
        </div>
      </div>
    </StaffContext.Provider>
  );
}
