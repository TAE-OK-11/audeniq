import { useRef, useState, type ReactNode } from 'react';
import { Link, useNavigate } from '../lib/router';
import { fixPath, resolveCorrection, splitCorrections } from '../lib/corrections';
import { api } from '../api/client';
import { STATUS_LABEL, money, num, statusTone } from '../lib/format';
import { useProfile } from '../store/profile';
import { isPaymentRegistered, usePayment } from '../store/payment';
import { useAccount } from '../store/account';
import { useDocs } from '../store/docs';
import { useBalance } from '../store/settlement';
import { useAsync } from '../hooks/useAsync';
import { SkeletonBlock, SkeletonRows } from '../components/Skeleton';
import { useSmoothHeight } from '../hooks/useSmoothHeight';
import { prefetchRelease } from '../api/prefetch';
import { Badge, CountUp, EmptyState, KeyValue, Steps } from '../ui';
import { Money } from '../components/Money';
import { ReleaseCover } from '../components/ReleaseCover';
import { latestSummary, periodLabel } from '../data/reports';
import { useReportRows } from '../hooks/useReportRows';
import type { CSSVarStyle } from '../hooks/useAnimations';
import { useGrowOnView } from '../hooks/useAnimations';
import { prefetchRoute } from '../routes';
import { CheckIcon } from '../components/Check';
import { Glyph } from '../components/Glyph';
import { josa } from '../lib/josa';
import { RELEASE_STEPS, dDayLabel, releaseStep, upcomingRelease } from '../lib/home';
import { useHomeSignals } from '../hooks/useHomeSignals';
import { MOCK } from '../lib/mode';
import type { Release } from '../api/types';

type GridKey = 'releases' | 'reports' | 'settlement' | 'contracts';

const GRID_ITEMS: { key: GridKey; to: string; title: string; desc: string; icon: ReactNode }[] = [
  {
    key: 'releases', to: '/releases', title: '발매·곡 관리', desc: '발매 목록과 곡별 정보',
    icon: <Glyph name="release" size={22} />,
  },
  {
    key: 'reports', to: '/reports', title: '음악 리포트', desc: '플랫폼별 재생과 수익 내역',
    icon: <Glyph name="chart" size={22} />,
  },
  {
    key: 'settlement', to: '/settlement', title: '정산·지급', desc: '정산 내역과 지급 요청',
    icon: <Glyph name="won" size={22} />,
  },
  {
    key: 'contracts', to: '/contracts', title: '계약서·권리', desc: '계약 상태와 증빙 서류',
    icon: <Glyph name="doc" size={22} />,
  },
];

const TASK_PREVIEW = 4;

const TASK_ICONS = {
  alert: <Glyph name="warning" size={20} />,
  doc: <Glyph name="doc" size={20} />,
  pen: <Glyph name="sign" size={20} />,
  draft: <Glyph name="pencil" size={20} />,
  user: <Glyph name="user" size={20} />,
  bell: <Glyph name="bell" size={20} />,
  won: <Glyph name="won" size={20} />,
  lock: <Glyph name="lock" size={20} />,
  star: <Glyph name="star" size={20} />,
  users: <Glyph name="users" size={20} />,
  approval: <Glyph name="approval" size={20} />,
};

function greeting(): string {
  const h = new Date().getHours();
  if (h < 6) return '늦은 밤에도 반가워요.';
  if (h < 12) return '좋은 아침이에요.';
  if (h < 18) return '좋은 오후예요.';
  return '오늘도 수고했어요.';
}

/** 9월 26일 토요일 */
function todayLabel(): string {
  const d = new Date();
  return `${d.getMonth() + 1}월 ${d.getDate()}일 ${'일월화수목금토'[d.getDay()]}요일`;
}

/** 막대 위 짧은 금액 — 1만 원 이상은 ‘2.7만’처럼 */
const shortWon = (n: number) => (n >= 10000 ? `${(n / 10000).toFixed(n >= 100000 ? 0 : 1)}만` : num(n));

function MiniTrend({ data }: { data: { period: string; revenue: number }[] }) {
  const ref = useGrowOnView<HTMLDivElement>('.aq-mini-bar');
  const high = Math.max(1, ...data.map(d => d.revenue));
  return (
    <div className="aq-mini-trend" ref={ref} role="img" aria-label="최근 월별 수익 추이">
      {data.map((d, i) => (
        <span key={d.period} className="aq-mini-col">
          <b className="aq-mini-val">{shortWon(d.revenue)}</b>
          <span
            className={`aq-mini-bar${i === data.length - 1 ? ' is-current' : ''}`}
            style={{ '--h': `${Math.max(6, Math.round((d.revenue / high) * 100))}%`, '--d': `${i * 80}ms` } as CSSVarStyle}
            title={`${periodLabel(d.period)} · ${money(d.revenue)}`}
          />
          <small>{Number(d.period.slice(5))}월</small>
        </span>
      ))}
    </div>
  );
}

/** 정산 — 받을 수 있는 금액과 다음 할 일(계좌 등록·지급 요청). 아직 돈이 하나도 없으면 숨긴다 */
function MoneyCard({ payable, pending, held, minimum, registered }: {
  payable: number; pending: number; held: number; minimum: number; registered: boolean;
}) {
  const navigate = useNavigate();
  // 체험 모드의 used는 지난 지급 요청 합계라 '처리 중'이 아니다
  const processing = MOCK ? 0 : pending;
  if (payable <= 0 && processing <= 0 && held <= 0) return null;
  const canRequest = registered && payable >= minimum && payable > 0;
  const hint = !registered && payable > 0 ? '계좌를 등록하면 지급을 요청할 수 있어요.'
    : registered && payable > 0 && payable < minimum ? `${money(minimum)}부터 지급을 요청할 수 있어요.`
    : [processing > 0 && `지급 처리 중 ${money(processing)}`, held > 0 && `보류 ${money(held)}`].filter(Boolean).join(' · ') || undefined;
  // 계좌 등록은 '확인할 작업'에 이미 있다 — 여기서는 지급 요청만 (한 화면에 같은 일을 두 번 두지 않는다).
  // 주 실행(파란) 버튼은 맨 위 큰 카드 하나뿐 — 여기는 보조 버튼 (한 화면에 판단 하나)
  const action = canRequest ? { to: '/settlement', label: '지급 요청하기' } : undefined;
  return (
    <section className="ui-card aq-home-money" aria-labelledby="moneyTitle">
      <div className="ui-section-head">
        <h2 id="moneyTitle">정산</h2>
        <button type="button" className="aq-head-link" onClick={() => navigate('/settlement')}>정산 보기<Glyph name="chevron-right" size={14} /></button>
      </div>
      <div className="aq-report-total">
        <small>받을 수 있는 금액</small>
        <strong><Money value={payable} animate /></strong>
        {hint && <p className="ui-row-meta">{hint}</p>}
      </div>
      {action && <Link className="ui-btn is-white is-block" to={action.to} onPointerEnter={() => prefetchRoute(action.to)}>{action.label}</Link>}
    </section>
  );
}

/** 다가오는 발매 — '내 발매'와 같은 줄에 남은 날, 아래에 지금 단계 (접수 → 검토 → 배급 승인 → 발매) */
function UpcomingCard({ release: r }: { release: Release }) {
  const live = r.livePlatforms?.length ?? 0;
  const note = r.status === 'live' ? (live > 0 ? `플랫폼 ${live}곳에 올라갔어요.` : '')
    : r.status === 'scheduled' ? '배급을 승인했어요. 발매일에 맞춰 플랫폼에 보내요.'
    : r.status === 'needs' ? '보완하면 검토를 이어서 해요.' : '';
  return (
    <section className="ui-card aq-home-upcoming" aria-labelledby="nextTitle">
      <div className="ui-section-head">
        <h2 id="nextTitle">{r.status === 'live' ? '최근 발매' : '다가오는 발매'}</h2>
      </div>
      {/* 안쪽에 흰 상자를 또 두지 않는다 — 카드 면 위에 바로 놓아야 여백이 생기고 덜 답답하다 */}
      <Link to={`/releases/${r.id}`} className="aq-upcoming-link aq-row-link" onPointerDown={() => prefetchRelease(r.id)}>
        <ReleaseCover id={r.id} src={r.coverData} />
        <div className="min-0">
          <span className="ui-row-title">{r.title || '제목 없는 발매'}</span>
          <span className="ui-row-meta">{r.artist || '아티스트 없음'} · {r.track_count}곡 · {r.release_date || '발매일 아직 없음'}</span>
        </div>
        {r.release_date && <Badge tone={r.status === 'live' ? 'green' : r.status === 'needs' ? 'amber' : 'blue'}>{dDayLabel(r.release_date)}</Badge>}
      </Link>
      <Steps steps={RELEASE_STEPS.map((s, i) => (i === releaseStep(r.status) && r.status === 'needs' ? '보완 필요' : s))} current={releaseStep(r.status)} warn={r.status === 'needs'} label="발매 진행 단계" />
      {note && <p className="ui-row-meta">{note}</p>}
    </section>
  );
}

export function Dashboard() {
  const navigate = useNavigate();
  const profile = useProfile();
  const docs = useDocs();
  const payment = usePayment();
  const account = useAccount();
  const { left: payable, used: payoutPending, held, minimum } = useBalance();
  // 할 일은 앞의 몇 개만 — 나머지는 '더 보기'로 (한 화면에 일곱 줄씩 쌓이면 무엇부터 할지 흐려진다)
  const [allTasks, setAllTasks] = useState(false);
  const { data: releases = [], loading, error, reload } = useAsync(() => api.listReleases(), [], { key: 'releases' });
  const topRef = useRef<HTMLDivElement>(null);
  useSmoothHeight(topRef, loading);
  const { rows: reportRows } = useReportRows();
  const report = latestSummary(reportRows);
  const signals = useHomeSignals(releases, loading);
  const upcoming = loading ? undefined : upcomingRelease(releases);

  const needs = releases.filter(r => r.status === 'needs').length;
  const reviewing = releases.filter(r => r.status === 'review').length;
  const drafts = releases.filter(r => r.status === 'draft').length;
  const toSign = docs.filter(d => d.kind === 'agreements' && d.reviewStatus === 'approved' && !d.localSignatureAt).length;
  const docFix = docs.filter(d => d.kind === 'rights' && d.reviewStatus === 'needs').length;

  const profileName = profile.name.trim();
  type TaskTone = 'warn' | 'info';
  const tasks: { icon: keyof typeof TASK_ICONS; name: string; sub: string; btn: string; to: string; tone: TaskTone }[] = [];
  if (needs) {
    // 한 건이면 신청서의 보완할 입력칸으로 바로
    const only = needs === 1 ? releases.find(r => r.status === 'needs') : undefined;
    const first = splitCorrections(only?.corrections).items[0];
    tasks.push({
      icon: 'alert', name: `보완이 필요한 발매 ${needs}건`,
      sub: only ? `‘${only.title}’ · ${josa(first ? resolveCorrection(first).label : '요청 내용', '을/를')} 고쳐 주세요.` : '요청 항목을 고쳐서 다시 접수해 주세요.',
      btn: '보완', to: only ? fixPath(only.id, first) : '/releases?status=needs', tone: 'warn',
    });
  }
  if (docFix) tasks.push({ icon: 'doc', name: `보완 요청된 서류 ${docFix}건`, sub: '요청 사유를 보고 새 원본을 제출해 주세요.', btn: '제출', to: '/rights', tone: 'warn' });
  if (toSign) tasks.push({ icon: 'pen', name: `서명할 계약서 ${toSign}건`, sub: '검토가 끝난 계약서에 서명해 주세요.', btn: '서명', to: '/contracts', tone: 'info' });
  // 수익을 받으려면 꼭 필요한 두 가지 — 예전엔 메뉴 안에서만 알려 줘서 놓치기 쉬웠다. 누르면 그 창이 바로 열린다
  if (!isPaymentRegistered(payment)) tasks.push({ icon: 'won', name: '정산 계좌 등록', sub: '수익을 받을 계좌를 등록해 주세요. 2분이면 돼요.', btn: '등록', to: '/profile?open=payout', tone: 'info' });
  if (!account.verified) tasks.push({ icon: 'lock', name: '본인인증', sub: '정산과 계약 서명에 필요해요. 예금주와 같은 이름이어야 해요.', btn: '인증', to: '/profile?open=verify', tone: 'info' });
  // 마감이 있는 일 — 피칭은 발매일 며칠 전에 닫힌다. 사흘 안이면 맨 위 큰 카드로
  if (signals.pitch) {
    const p = signals.pitch;
    tasks.push({
      icon: 'star', name: `플레이리스트 피칭 마감 ${p.daysLeft ? `D-${p.daysLeft}` : '오늘'}`,
      sub: `‘${p.title}’ 대표곡을 플랫폼 에디터에게 소개할 수 있어요.`,
      btn: '피칭', to: `/releases/${p.releaseId}`, tone: p.daysLeft <= 3 ? 'warn' : 'info',
    });
  }
  if (signals.ai) tasks.push({ icon: 'approval', name: `AI 앱 승인 요청 ${signals.ai.count}건`, sub: '승인하기 전에는 아무것도 바뀌지 않아요.', btn: '확인', to: signals.ai.to, tone: 'info' });
  if (signals.splits.length) {
    const waiting = signals.splits.reduce((n, s) => n + s.waiting, 0);
    const one = signals.splits.length === 1 ? signals.splits[0] : undefined;
    tasks.push({
      icon: 'users', name: `수익 분배 수락 대기 ${waiting}명`,
      sub: one ? `‘${one.title}’ 공동작업자가 아직 수락하지 않았어요. 메일을 다시 보낼 수 있어요.` : '공동작업자가 수락하면 그분 몫이 바로 정산돼요.',
      btn: '확인', to: one ? `/releases/${one.releaseId}` : '/releases', tone: 'info',
    });
  }
  if (drafts) tasks.push({ icon: 'draft', name: `작성 중인 발매 ${drafts}건`, sub: '이어서 작성하고 접수해 보세요.', btn: '보기', to: '/releases?status=draft', tone: 'info' });
  if (!profileName) tasks.push({ icon: 'user', name: '계정 정보 등록', sub: '활동명과 연락처를 입력해 주세요.', btn: '등록', to: '/profile', tone: 'info' });
  // 읽지 않은 알림은 머리의 종 숫자로 이미 보인다 — 할 일 목록에서는 뺀다 (홈이 너무 길고 복잡해 보였다)

  // 정산·재생이 아직 하나도 없으면 빈 차트 대신 짧은 안내만
  const reportEmpty = report.revenue <= 0 && report.plays <= 0 && report.trend.every(d => !d.revenue);

  // 보완처럼 기한이 걸린 일이 있으면 맨 위 큰 카드를 그 일로 — '새 발매'보다 먼저 할 일이다 (한 화면에 판단 하나)
  const urgent = loading ? undefined : tasks.find(t => t.tone === 'warn');
  const restTasks = urgent ? tasks.filter(t => t !== urgent) : tasks;

  // 바로가기는 메뉴와 같은 이름만 두지 않고 지금 상태를 한 줄로 — 들어가기 전에 무엇이 있는지 보인다
  const gridStatus: Partial<Record<GridKey, { text: string; tone?: 'warn' }>> = {};
  if (!loading) {
    if (needs) gridStatus.releases = { text: `보완 필요 ${needs}건`, tone: 'warn' };
    else if (reviewing) gridStatus.releases = { text: `검토 중 ${reviewing}건` };
    else if (releases.length) gridStatus.releases = { text: `발매 ${releases.length}건` };
  }
  if (!reportEmpty && report.revenue > 0) gridStatus.reports = { text: `${Number(report.period.slice(5))}월 수익 ${money(report.revenue)}` };
  if (docFix) gridStatus.contracts = { text: `보완 서류 ${docFix}건`, tone: 'warn' };
  else if (toSign) gridStatus.contracts = { text: `서명할 계약서 ${toSign}건` };

  return (
    <div id="view-home" className="view">
      {/* 머리와 큰 카드는 불러오기가 끝나면 높이가 바뀐다(급한 일이 있을 때만 '새 발매' 버튼, 자리표시보다 큰 카드) —
          한 번에 아래 내용을 밀지 않고 부드럽게 자란다 */}
      <div ref={topRef} className="aq-home-top">
      <div className="ui-page-head aq-home-title">
        <div>
          <p className="aq-home-date">{todayLabel()}</p>
          <h1>{profileName ? <>{profileName}<span className="aq-home-suffix">님의 작업실</span></> : '내 작업실'}</h1>
          <p>{greeting()} 오늘 확인할 작업을 모아 두었어요.</p>
        </div>
        {urgent && (
          <Link className="ui-btn is-soft is-sm aq-home-new" to="/upload" onPointerEnter={() => prefetchRoute('/upload')}><Glyph name="plus" size={14} />새 발매</Link>
        )}
      </div>

      {loading ? (
        <SkeletonBlock height={160} className="aq-home-skel" />
      ) : urgent ? (
        <Link className="ui-hero aq-home-hero ui-tone-warn is-urgent" to={urgent.to} onPointerEnter={() => prefetchRoute(urgent.to)}>
          <div>
            <p className="eyebrow">먼저 할 일</p>
            <h2>{urgent.name}</h2>
            <p>{urgent.sub}</p>
          </div>
          <span className="ui-btn is-primary aq-hero-cta">
            {urgent.btn}하기
            <span className="aq-cta-arrow" aria-hidden="true">
              <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M3 8h10M9 4l4 4-4 4" /></svg>
            </span>
          </span>
        </Link>
      ) : (
      <Link className="ui-hero aq-home-hero" to="/upload" onPointerEnter={() => prefetchRoute('/upload')}>
        <div>
          <p className="eyebrow">새 발매</p>
          <h2>새로운 발매를 <span className="aq-accent">시작해 보세요.</span></h2>
          <p>음원과 커버아트, 크레딧을 등록하고 발매를 준비해요.</p>
        </div>
        <span className="ui-btn is-primary aq-hero-cta">
          발매 등록하기
          <span className="aq-cta-arrow" aria-hidden="true">
            <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M3 8h10M9 4l4 4-4 4" /></svg>
          </span>
        </span>
      </Link>
      )}
      </div>

      {/* 자주 가는 화면 바로가기는 새 발매 바로 아래 — 할 일·발매 목록 아래로 내리면 누르러 한참 내려가야 했다.
          왼쪽 메뉴가 같은 곳을 배지와 함께 보여 주는 넓은 화면에서는 숨긴다 */}
      <div className="dashboard-grid aq-stagger aq-home-shortcuts" aria-label="주요 업무">
        {GRID_ITEMS.map(item => {
          const st = gridStatus[item.key];
          return (
          <Link key={item.to} className="service-item" to={item.to} onPointerEnter={() => prefetchRoute(item.to)} onFocus={() => prefetchRoute(item.to)}>
            <span className="ui-icon">{item.icon}</span>
            <div>
              <h3>{item.title}</h3>
              <p className={st ? `aq-grid-status${st.tone ? ` is-${st.tone}` : ''}` : undefined}>{st?.text ?? item.desc}</p>
            </div>
          </Link>
          );
        })}
      </div>

      {/* 넓은 화면은 두 줄기 — 왼쪽은 할 일·내 발매(길다), 오른쪽은 다가오는 발매·정산·리포트(짧다)를 쌓아
          짧은 카드 옆에 빈 칸이 생기지 않게. 좁은 화면은 한 줄로: 할 일 → 다가오는 발매 → 내 발매 → 정산 → 리포트 */}
      <div className="dashboard-columns aq-home-board">
        <div className="aq-home-main">
          {/* 해야 할 일을 먼저 — 휴대폰에서 '확인할 작업'이 발매 목록 아래(약 1100px)에 묻혀 있었다 */}
          <section className="ui-card aq-home-tasks" aria-labelledby="workTitle">
            <div className="ui-section-head">
              <h2 id="workTitle">확인할 작업</h2>
              {restTasks.length > 0 && <span className="ui-count is-soft">{restTasks.length}</span>}
            </div>
            <div id="homeTasks">
              {loading ? (
                <SkeletonRows count={2} />
              ) : restTasks.length ? (
                <>
                <ul className="aq-task-list aq-stagger">
                  {(allTasks ? restTasks : restTasks.slice(0, TASK_PREVIEW)).map((t, i) => (
                    // '더 보기'로 붙는 줄은 높이가 자라며 펼쳐지고 접을 때 줄어든다 — 아래 '내 발매'가 툭 밀리지 않게 (lib/expand.ts)
                    <li key={t.name} data-expand={i >= TASK_PREVIEW || undefined}>
                      <button type="button" className={`aq-task is-${t.tone}`} onClick={() => navigate(t.to)}>
                        <span className="ui-icon" aria-hidden="true">{TASK_ICONS[t.icon]}</span>
                        <span className="aq-task-text">
                          <strong>{t.name}</strong>
                          <small>{t.sub}</small>
                        </span>
                        <span className="aq-task-go">{t.btn}<i aria-hidden="true"><Glyph name="chevron-right" size={14} /></i></span>
                      </button>
                    </li>
                  ))}
                </ul>
                {restTasks.length > TASK_PREVIEW && (
                  <button type="button" className="aq-task-more" aria-expanded={allTasks} onClick={() => setAllTasks(v => !v)}>
                    {allTasks ? '접기' : `${restTasks.length - TASK_PREVIEW}건 더 보기`}
                    <Glyph name="chevron-down" size={14} className={allTasks ? 'is-flip' : ''} />
                  </button>
                )}
                </>
              ) : (
                <div className="empty-note aq-all-done">
                  <span className="aq-check-badge"><CheckIcon size={12} /></span>
                  {urgent ? '그 밖에 확인할 작업은 없어요.' : '지금 확인할 작업이 없어요.'}
                </div>
              )}
            </div>
          </section>
          <section className="ui-card aq-home-releases" aria-labelledby="upcomingTitle">
            <div className="ui-section-head">
              <h2 id="upcomingTitle">내 발매</h2>
              <button type="button" className="aq-head-link" onClick={() => navigate('/releases')}>전체 보기<Glyph name="chevron-right" size={14} /></button>
            </div>
            <div id="homeReleases" aria-live="polite">
              {loading ? (
                <SkeletonRows count={3} />
              ) : error ? (
                <div className="empty-note">
                  {error}<br />
                  <button className="ui-btn is-white is-sm" type="button" onClick={reload}>다시 불러오기</button>
                </div>
              ) : releases.length ? (
                <div className="ui-list aq-stagger">
                  {releases.slice(0, 4).map(r => (
                    <Link key={r.id} to={`/releases/${r.id}`} className="release-row ui-row studio-album-row aq-row-link" onPointerDown={() => prefetchRelease(r.id)}>
                      <ReleaseCover id={r.id} src={r.coverData} />
                      <div className="min-0">
                        <span className="ui-row-title">{r.title || '제목 없는 발매'}</span>
                        <span className="ui-row-meta">{r.artist || '아티스트 없음'} · {r.track_count}곡 · {r.release_date || '발매일 아직 없음'}</span>
                      </div>
                      <div className="ui-row-end">
                        <Badge tone={statusTone(r.status)} className={`aq-status is-${r.status}`}>{STATUS_LABEL[r.status] || r.status}</Badge>
                      </div>
                    </Link>
                  ))}
                </div>
              ) : (
                <div className="empty-note">
                  아직 등록한 발매가 없어요.<br />새 발매를 만들면 여기에서 확인할 수 있어요.<br />
                  <button className="ui-btn is-white is-sm" type="button" onClick={() => navigate('/upload')}>발매 등록하기 <Glyph name="chevron-right" size={13} /></button>
                </div>
              )}
            </div>
          </section>
        </div>
        <div className="aq-home-side">
            {!loading && upcoming && <UpcomingCard release={upcoming} />}
            {!loading && <MoneyCard payable={payable} pending={payoutPending} held={held} minimum={minimum} registered={isPaymentRegistered(payment)} />}
          {/* 리포트 — 제목은 짧게(기간은 보조 글자), 금액 옆에 전월 대비, 아래에 세 숫자. 모양은 Badge · KeyValue · EmptyState */}
          <section className="ui-card aq-report-card" aria-labelledby="reportTitle">
            <div className="ui-section-head aq-report-head">
              <h2 id="reportTitle">음악 리포트{!reportEmpty && <small>{periodLabel(report.period)}</small>}</h2>
              <button className="aq-head-link" type="button" onClick={() => navigate('/reports')}>리포트 보기<Glyph name="chevron-right" size={14} /></button>
            </div>
            {reportEmpty ? (
              <EmptyState compact icon={<Glyph name="chart" size={22} />} title="아직 수익 리포트가 없어요">
                발매가 플랫폼에 나가고 정산이 들어오면 여기서 수익과 재생 수를 보여 드려요.
              </EmptyState>
            ) : (<>
            <div className="aq-report-total">
              <small>수익</small>
              <div className="aq-report-amount">
                <strong><Money value={report.revenue} animate /></strong>
                {report.change != null && (
                  <Badge tone={report.change < 0 ? 'red' : 'blue'}>전월 대비 {report.change >= 0 ? '+' : ''}{report.change.toFixed(1)}%</Badge>
                )}
              </div>
            </div>
            <MiniTrend data={report.trend} />
            <KeyValue className="aq-report-kv" items={[
              ['총 재생', report.plays > 0 ? <CountUp value={report.plays} format={n => `${num(n)}회`} /> : '아직 없어요'],
              ['많이 들은 곡', report.topTrack || '—'],
              ['많이 들은 플랫폼', report.topPlatform || '—'],
            ]} />
            </>)}
          </section>
        </div>
      </div>
    </div>
  );
}
