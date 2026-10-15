// AUDENIQ ADMIN — 스태프 전용 화면 (/admin/*). 스튜디오와 같은 로그인 세션을 쓰고,
// 스태프 역할(ADMIN·REVIEWER·OPERATOR·SUPPORT)은 서버 `/api/staff/me`가 알려준다.
import { Suspense, lazy, useCallback, useEffect, useMemo, useState, type ComponentType, type ReactNode } from 'react';
import { Link, Navigate, Route, Routes, useLocation, useNavigate } from '../lib/router';
import { useAuth } from '../api/auth';
import { ApiError } from '../api/errors';
import { MOCK } from '../lib/mode';
import { useConfirm } from '../components/Confirm';
import { useToast } from '../components/Toast';
import { staffApi, type Duty, type Overview, type StaffMe } from './api';
import { ROLE_LABEL } from './labels';
import { SkeletonList, StaffContext, WORK_PATHS } from './ui';
import { OverviewPage } from './pages/Overview';

// 첫 화면(오늘의 업무) 말고는 열 때 불러온다 — 첫 로딩 JS를 줄이고, 로그인 뒤 한가할 때 미리 받아 둔다
type PageModule = Record<string, ComponentType>;
const PAGE_LOADERS = {
  ReviewQueue: () => import('./pages/ReviewQueue'),
  ReviewDetail: () => import('./pages/ReviewDetail'),
  Approvals: () => import('./pages/Approvals'),
  Documents: () => import('./pages/Documents'),
  Inquiries: () => import('./pages/Inquiries'),
  Deliveries: () => import('./pages/Deliveries'),
  Dsps: () => import('./pages/Dsps'),
  Takedowns: () => import('./pages/Takedowns'),
  Corrections: () => import('./pages/Corrections'),
  Pitches: () => import('./pages/Pitches'),
  PlatformRequests: () => import('./pages/PlatformRequests'),
  Terms: () => import('./pages/Terms'),
  Claims: () => import('./pages/Claims'),
  Payouts: () => import('./pages/Payouts'),
  ContentAdmin: () => import('../pages/ContentAdmin'),
  Tasks: () => import('./work/pages/Tasks'),
  TaskDetail: () => import('./work/pages/TaskDetail'),
  Inbox: () => import('./work/pages/Inbox'),
  Exceptions: () => import('./work/pages/Exceptions'),
  Sla: () => import('./work/pages/Sla'),
  Crm: () => import('./work/pages/Crm'),
  Chat: () => import('./work/pages/Chat'),
  Docs: () => import('./work/pages/Docs'),
  Board: () => import('./work/pages/Board'),
  CrmDetail: () => import('./work/pages/Crm'),
  DspOps: () => import('./work/pages/DspOps'),
} satisfies Record<string, () => Promise<unknown>>;
const page = (name: keyof typeof PAGE_LOADERS) =>
  lazy(() => PAGE_LOADERS[name]().then(m => ({ default: (m as unknown as PageModule)[name] })));
const ReviewQueue = page('ReviewQueue');
const ReviewDetail = page('ReviewDetail');
const Approvals = page('Approvals');
const Documents = page('Documents');
const Inquiries = page('Inquiries');
const Deliveries = page('Deliveries');
const Dsps = page('Dsps');
const Takedowns = page('Takedowns');
const Corrections = page('Corrections');
const Pitches = page('Pitches');
const PlatformRequests = page('PlatformRequests');
const Terms = page('Terms');
const Claims = page('Claims');
const Payouts = page('Payouts');
const ContentAdmin = page('ContentAdmin');
const Tasks = page('Tasks');
const TaskDetail = page('TaskDetail');
const Inbox = page('Inbox');
const Exceptions = page('Exceptions');
const Sla = page('Sla');
const Crm = page('Crm');
const Chat = page('Chat');
const Docs = page('Docs');
const Board = page('Board');
const CrmDetail = page('CrmDetail');
const DspOps = page('DspOps');
// 디자인 시스템 화면은 미리 받지 않는다 (가끔 여는 참고 화면)
const DesignSystem = lazy(() => import('./pages/DesignSystem').then(m => ({ default: m.DesignSystem })));
function prefetchPages() {
  const run = () => { for (const load of Object.values(PAGE_LOADERS)) void load().catch(() => {}); };
  if ('requestIdleCallback' in window) window.requestIdleCallback(run, { timeout: 4000 });
  else setTimeout(run, 1500);
}
import '../styles/admin.css';
import '../styles/work.css';
import '../styles/admin-refresh.css';
import { Glyph } from '../components/Glyph';
import { openLive } from '../lib/live';
import { alertsBlocked, alertsEnabled, alertsSupported, disableAlerts, enableAlerts, systemAlert } from '../lib/alerts';
import { ADMIN_INQUIRIES_EVENT, ADMIN_WORK_EVENT } from './liveEvents';
import { workApi } from './work/api';
import type { WorkSummary } from './work/types';
import { Avatar, Badge } from '../ui';

// 관리자 메뉴 아이콘 — 스튜디오와 같은 AUDENIQ 아이콘 세트(Glyph)
const ICONS: Record<string, string> = {
  home: 'home', review: 'review', approval: 'approval', doc: 'doc', inquiry: 'inquiry',
  delivery: 'truck', dsp: 'globe', payout: 'card',
  task: 'task', inbox: 'bell', exception: 'warning', sla: 'clock', crm: 'users', chat: 'inquiry', design: 'layers',
};

export function Icon({ name }: { name: string }) {
  return <Glyph name={ICONS[name] ?? name} size={18} />;
}

interface NavItem { to: string; label: string; icon: string; count?: (o: Overview | null, w: WorkSummary | null) => number; hot?: boolean; adminOnly?: boolean; also?: string[] }
const NAV: { group: string; items: NavItem[] }[] = [
  { group: '한눈에', items: [{ to: '/admin', label: '오늘의 업무', icon: 'home' }] },
  {
    group: '업무',
    items: [
      // 업무함 · 업무 · 예외 처리 · 관계 관리 · SLA는 한 메뉴(업무) 안의 탭
      { to: '/admin/inbox', label: '업무', icon: 'task', count: (_, w) => w?.inbox.unread ?? 0, hot: true, also: WORK_PATHS },
      { to: '/admin/chat', label: '팀 대화', icon: 'chat', count: (_, w) => w?.chat?.unread ?? 0 },
      { to: '/admin/docs', label: '문서', icon: 'doc' },
    ],
  },
  {
    group: '심사',
    items: [
      // 2차 승인은 발매 심사 안의 탭
      { to: '/admin/reviews', label: '발매 심사', icon: 'review', count: o => (o ? o.review + o.second_approvals : 0), hot: true, also: ['/admin/approvals'] },
      { to: '/admin/documents', label: '서류 검토', icon: 'doc', count: o => (o ? o.documents : 0) },
    ],
  },
  { group: '고객 지원', items: [{ to: '/admin/inquiries', label: '문의 답변', icon: 'inquiry', count: o => (o ? o.inquiries : 0), hot: true }] },
  { group: '콘텐츠', items: [{ to: '/admin/content', label: '공지·이벤트 관리', icon: 'doc', adminOnly: true }] },
  {
    group: '배급',
    items: [
      // 배급은 최종 승인 + 아티스트 서명 후 자동 — 여기서는 현황과 문제만 본다 (DSP 사양은 탭)
      { to: '/admin/deliveries', label: '배급 현황', icon: 'delivery', count: o => (o ? o.deliveries_blocked : 0), hot: true, also: ['/admin/dsps', '/admin/dsp-ops', '/admin/takedowns', '/admin/claims', '/admin/corrections', '/admin/pitches', '/admin/platform-requests', '/admin/terms'] },
      { to: '/admin/payouts', label: '지급 요청', icon: 'payout', count: o => (o ? o.payout_requests : 0), adminOnly: true },
    ],
  },
  { group: '시스템', items: [{ to: '/admin/design', label: '디자인 시스템', icon: 'design' }] },
];

const active = (path: string, to: string) => (to === '/admin' ? path === '/admin' || path === '/admin/' : path === to || path.startsWith(`${to}/`));

function Gate({ title, children, action }: { title: string; children: ReactNode; action?: ReactNode }) {
  return (
    <div className="adm-gate">
      <div className="ui-icon is-lg" aria-hidden="true"><Glyph name="lock" size={26} /></div>
      <h1>{title}</h1>
      <p>{children}</p>
      {action}
    </div>
  );
}

/** 브라우저 알림 — 다른 탭에 있어도 새 고객 메시지를 알려 준다 */
function AlertToggle() {
  const toast = useToast();
  const [on, setOn] = useState(alertsEnabled);
  if (!alertsSupported() || MOCK) return null;
  const toggle = async () => {
    if (on) { disableAlerts(); setOn(false); toast('브라우저 알림을 껐어요.', 'info'); return; }
    if (alertsBlocked()) { toast('브라우저 설정에서 이 사이트의 알림을 허용해 주세요.', 'error'); return; }
    const ok = await enableAlerts();
    setOn(ok);
    toast(ok ? '브라우저 알림을 켰어요. 새 고객 메시지를 알려 드릴게요.' : '브라우저에서 알림 권한을 허용해 주세요.', ok ? 'success' : 'error');
  };
  return (
    <button type="button" className={`ui-btn is-ghost is-sm adm-alert-toggle${on ? ' is-on' : ''}`} aria-pressed={on} onClick={toggle} title={on ? '브라우저 알림 끄기' : '브라우저 알림 켜기'}>
      <Glyph name="bell" size={15} /><span className="adm-ghost-text">{on ? '알림 켜짐' : '알림 켜기'}</span>
    </button>
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
  const [work, setWork] = useState<WorkSummary | null>(null);

  const loadMe = useCallback(() => {
    setGate('loading');
    staffApi.me()
      .then(m => { setMe(m); setGate('ok'); prefetchPages(); })
      .catch(e => setGate(e instanceof ApiError && e.status === 403 ? 'forbidden' : 'error'));
  }, []);
  useEffect(loadMe, [loadMe]);

  const refreshCounts = useCallback(() => {
    staffApi.overview().then(setCounts).catch(() => { /* 숫자는 보조 정보 — 실패해도 화면은 쓸 수 있다 */ });
    workApi.summary().then(setWork).catch(() => { /* 업무함 숫자도 보조 정보 */ });
  }, []);
  useEffect(() => {
    if (gate !== 'ok') return;
    refreshCounts();
    // 여러 담당자가 동시에 처리하므로 1분마다 대기 건수를 새로 받는다 (탭이 보일 때만)
    const t = window.setInterval(() => { if (document.visibilityState === 'visible') refreshCounts(); }, 60_000);
    // 실시간 연결 — 고객 문의·업무가 바뀌면 숫자와 열린 화면을 바로 새로 고치고, 새 고객 메시지는 알린다
    const stopLive = MOCK ? () => {} : openLive('/api/staff/live', {
      on: {
        inquiries: d => {
          refreshCounts();
          window.dispatchEvent(new CustomEvent(ADMIN_INQUIRIES_EVENT, { detail: { inquiryId: d.inquiry_id ?? null } }));
          if (d.customer && me?.duties.includes('INQUIRIES') && d.inquiry_id) {
            const title = '새 고객 문의 메시지';
            const body = [d.org, d.subject].filter(Boolean).join(' · ');
            if (!systemAlert(title, body, `/admin/inquiries/${d.inquiry_id}`, `inq-${d.inquiry_id}`)) toast(`${title} — ${body}`, 'info');
          }
        },
        work: () => { refreshCounts(); window.dispatchEvent(new Event(ADMIN_WORK_EVENT)); },
      },
      resync: refreshCounts,
    });
    return () => { window.clearInterval(t); stopLive(); };
  }, [gate, refreshCounts]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => { window.scrollTo(0, 0); }, [loc.pathname]);
  useEffect(() => {
    const prev = document.title;
    document.title = 'AUDENIQ ADMIN';
    return () => { document.title = prev; };
  }, []);

  const ctx = useMemo(() => me && ({
    me, counts, work, refreshCounts, can: (d: Duty) => me.duties.includes(d),
  }), [me, counts, work, refreshCounts]);

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
      <Gate title="관리자 권한이 필요해요" action={<Link to="/" className="ui-btn is-primary">스튜디오로 돌아가기</Link>}>
        {user?.email ? `${user.email} 계정은` : '이 계정은'} 아직 관리자로 등록되지 않았어요. 운영 책임자에게 권한을 요청해 주세요.
      </Gate>
    );
  }
  if (gate === 'error' || !ctx || !me) {
    return (
      <Gate title="관리자 정보를 불러오지 못했어요" action={<button type="button" className="ui-btn is-primary" onClick={loadMe}>다시 시도</button>}>
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
            <Link to="/admin" className="adm-brand" aria-label="AUDENIQ ADMIN 홈">
              <img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" />
              <span className="ui-product">ADMIN</span>
            </Link>
            <div className="adm-header-right">
              {MOCK && <Badge tone="amber">체험 데이터</Badge>}
              <AlertToggle />
              <div className="adm-who" title={user?.email}>
                <Avatar name={initial} size="sm" />
                <span className="adm-who-text"><b>{user?.email}</b><small>{ROLE_LABEL[me.role]}</small></span>
              </div>
              <Link to="/" className="ui-btn is-ghost is-sm" aria-label="스튜디오로 이동">스튜디오<Glyph name="arrow-up-right" size={14} /></Link>
              <button type="button" className="ui-btn is-ghost is-sm" onClick={doLogout}>로그아웃</button>
            </div>
          </div>
        </header>

        <div className="adm-shell">
          <nav className="adm-side" aria-label="관리자 메뉴">
            {NAV.map(g => (
              <div className="adm-side-group" key={g.group}>
                <div className="adm-side-label">{g.group}</div>
                {g.items.filter(i => !i.adminOnly || me.role === 'ADMIN').map(i => {
                  const n = i.count ? i.count(counts, work) : 0;
                  const cur = active(loc.pathname, i.to) || !!i.also?.some(a => active(loc.pathname, a));
                  return (
                    <Link key={i.to} to={i.to} aria-current={cur ? 'page' : undefined}>
                      <span><Icon name={i.icon} />{i.label}</span>
                      {n > 0
                        ? <b className={`ui-count${i.hot ? ' is-hot' : ''}`}>{n > 99 ? '99+' : n}</b>
                        // 숫자를 받기 전에는 배지 자리를 비워 둔다 — 휴대폰 가로 탭에서 숫자가 뒤늦게 붙으며 뒤 탭들이 밀리지 않게
                        : i.count && (!counts || !work) && <b className="ui-count is-pending" aria-hidden="true">0</b>}
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
            <Suspense fallback={<SkeletonList rows={6} />}>
            <Routes>
              <Route path="/admin" element={<OverviewPage />} />
              <Route path="/admin/reviews" element={<ReviewQueue />} />
              <Route path="/admin/reviews/:id" element={<ReviewDetail />} />
              <Route path="/admin/approvals" element={<Approvals />} />
              <Route path="/admin/documents" element={<Documents />} />
              <Route path="/admin/inquiries" element={<Inquiries />} />
              <Route path="/admin/inquiries/:id" element={<Inquiries />} />
              <Route path="/admin/deliveries" element={<Deliveries />} />
              <Route path="/admin/dsps" element={<Dsps />} />
              <Route path="/admin/takedowns" element={<Takedowns />} />
              <Route path="/admin/corrections" element={<Corrections />} />
              <Route path="/admin/pitches" element={<Pitches />} />
              <Route path="/admin/platform-requests" element={<PlatformRequests />} />
              <Route path="/admin/terms" element={<Terms />} />
              <Route path="/admin/claims" element={<Claims />} />
              <Route path="/admin/work" element={<Navigate to="/admin/inbox" replace />} />
              <Route path="/admin/inbox" element={<Inbox />} />
              <Route path="/admin/exceptions" element={<Exceptions />} />
              <Route path="/admin/sla" element={<Sla />} />
              <Route path="/admin/crm" element={<Crm />} />
              <Route path="/admin/crm/:id" element={<CrmDetail />} />
              <Route path="/admin/dsp-ops" element={<DspOps />} />
              <Route path="/admin/design" element={<DesignSystem />} />
              <Route path="/admin/tasks" element={<Tasks />} />
              <Route path="/admin/board" element={<Board />} />
              <Route path="/admin/board/:id" element={<Board />} />
              <Route path="/admin/chat" element={<Chat />} />
              <Route path="/admin/chat/:id" element={<Chat />} />
              <Route path="/admin/docs" element={<Docs />} />
              <Route path="/admin/docs/:id" element={<Docs />} />
              <Route path="/admin/tasks/:id" element={<TaskDetail />} />
              <Route path="/admin/payouts" element={me.role === 'ADMIN' ? <Payouts /> : <Navigate to="/admin" replace />} />
              <Route path="/admin/content" element={me.role === 'ADMIN' ? <ContentAdmin /> : <Navigate to="/admin" replace />} />
              <Route path="/content-admin" element={<Navigate to="/admin/content" replace />} />
              <Route path="*" element={<Navigate to="/admin" replace />} />
            </Routes>
            </Suspense>
          </main>
        </div>
      </div>
    </StaffContext.Provider>
  );
}
