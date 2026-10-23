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
import { prefetchRelease } from '../api/prefetch';
import { Badge, CountUp } from '../ui';
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
import { useLandingReveal } from '../hooks/useLandingReveal';
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

const ARROW = <Glyph name="arrow-up-right" size={16} />;

/** 홍보 페이지 섹션 머리 — 파란 작은 분류 글자 + 큰 제목 (+ 오른쪽 링크) */
function Head({ eyebrow, title, sub, link, delay = 0 }: { eyebrow: string; title: ReactNode; sub?: ReactNode; link?: { to: string; label: string }; delay?: number }) {
  return (
    <div className="hm-head">
      <div className="min-0">
        <p className="hm-eyebrow" data-reveal data-delay={delay}>{eyebrow}</p>
        <h2 data-reveal data-delay={delay + 65}>{title}</h2>
        {sub && <p className="hm-sub" data-reveal data-delay={delay + 130}>{sub}</p>}
      </div>
      {link && <Link className="hm-link" to={link.to} onPointerEnter={() => prefetchRoute(link.to)}>{link.label}<Glyph name="arrow-right" size={14} /></Link>}
    </div>
  );
}

const STEP_NOTE = ['신청서를 받았어요', 'AUDENIQ이 확인해요', '플랫폼에 보낼 준비', '플랫폼에 공개'];

/** 다가오는 발매 — 홍보 페이지의 여정 줄(01 → 04)로 지금 단계 */
function Journey({ release: r }: { release: Release }) {
  const step = releaseStep(r.status);
  const live = r.livePlatforms?.length ?? 0;
  return (
    <section className="hm-section" aria-labelledby="hmNext">
      <Head eyebrow={r.status === 'live' ? '최근 발매' : '다가오는 발매'} title={<span id="hmNext">{r.title || '제목 없는 발매'}</span>}
        sub={<>{r.artist || '아티스트 없음'} · {r.track_count}곡{r.release_date && <> · <span className="hm-pill">{dDayLabel(r.release_date)}</span></>}</>}
        link={{ to: `/releases/${r.id}`, label: '자세히' }} />
      <ol className="hm-journey" aria-label="발매 진행 단계">
        {RELEASE_STEPS.map((label, i) => {
          const warn = i === step && r.status === 'needs';
          const cls = i < step ? 'is-done' : i === step ? (warn ? 'is-warn' : 'is-current') : '';
          return (
            <li key={label} className={cls} aria-current={i === step ? 'step' : undefined} data-reveal data-delay={i * 70}>
              <span>{i < step ? <CheckIcon size={12} /> : `0${i + 1}`}</span>
              <strong>{warn ? '보완 필요' : label}</strong>
              <small>{warn ? '고치면 검토를 이어서 해요' : i === 3 && r.status === 'live' && live ? `플랫폼 ${live}곳` : STEP_NOTE[i]}</small>
            </li>
          );
        })}
      </ol>
    </section>
  );
}

/** 정산 — 홍보 페이지의 남색 패널. 아직 돈이 하나도 없으면 숨긴다 */
function MoneyPanel({ payable, pending, held, minimum, registered }: { payable: number; pending: number; held: number; minimum: number; registered: boolean }) {
  // 체험 모드의 used는 지난 지급 요청 합계라 '처리 중'이 아니다
  const processing = MOCK ? 0 : pending;
  if (payable <= 0 && processing <= 0 && held <= 0) return null;
  const canRequest = registered && payable >= minimum && payable > 0;
  const note = !registered ? '계좌를 등록하면 지급을 요청할 수 있어요.'
    : payable < minimum ? `${money(minimum)}부터 지급을 요청할 수 있어요.`
    : '등록한 계좌로 지급을 요청할 수 있어요.';
  const pills = [processing > 0 && `지급 처리 중 ${money(processing)}`, held > 0 && `보류 ${money(held)}`].filter(Boolean) as string[];
  return (
    <section className="hm-navy" aria-labelledby="hmMoney" data-reveal>
      <p className="hm-eyebrow" id="hmMoney">받을 수 있는 금액</p>
      <strong className="hm-amount"><CountUp value={payable} format={num} /><small>원</small></strong>
      <p>{note}</p>
      {pills.length > 0 && <div className="hm-pills">{pills.map(t => <span key={t} className="hm-pill is-dark">{t}</span>)}</div>}
      <div className="hm-actions">
        {canRequest && <Link className="hm-button is-white" to="/settlement" onPointerEnter={() => prefetchRoute('/settlement')}>지급 요청하기<span>{ARROW}</span></Link>}
        <Link className="hm-link is-dark" to="/settlement">정산 보기<Glyph name="arrow-right" size={14} /></Link>
      </div>
    </section>
  );
}

export function Dashboard() {
  const rootRef = useRef<HTMLDivElement>(null);
  const navigate = useNavigate();
  const profile = useProfile();
  const docs = useDocs();
  const payment = usePayment();
  const account = useAccount();
  const { left: payable, used: payoutPending, held, minimum } = useBalance();
  // 할 일은 앞의 몇 개만 — 나머지는 '더 보기'로 (한 화면에 일곱 줄씩 쌓이면 무엇부터 할지 흐려진다)
  const [allTasks, setAllTasks] = useState(false);
  const { data: releases = [], loading, error, reload } = useAsync(() => api.listReleases(), [], { key: 'releases' });
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
  if (payable > 0) gridStatus.settlement = { text: `받을 수 있는 금액 ${money(payable)}` };
  if (docFix) gridStatus.contracts = { text: `보완 서류 ${docFix}건`, tone: 'warn' };
  else if (toSign) gridStatus.contracts = { text: `서명할 계약서 ${toSign}건` };

  useLandingReveal(rootRef, !loading);

  const name = profileName || '내';
  // 큰 제목의 파란 줄 — 지금 가장 먼저 볼 것 하나 (한 화면에 판단 하나)
  const accent = loading ? '' : urgent ? `${urgent.name}.`
    : upcoming?.release_date && upcoming.status !== 'live' ? `${upcoming.title || '새 발매'}, ${dDayLabel(upcoming.release_date)}.`
    : '새 발매를 시작해요.';
  const desc = urgent ? urgent.sub
    : upcoming && upcoming.status !== 'live' ? '검토와 배급 승인을 마치면 발매일에 맞춰 플랫폼에 보내요.'
    : `${greeting()} 음원과 커버아트, 크레딧을 등록하고 발매를 준비해요.`;
  const cta = urgent ? { to: urgent.to, label: `${urgent.btn}하기` } : { to: '/upload', label: '발매 등록하기' };
  const shownTasks = allTasks ? restTasks : restTasks.slice(0, TASK_PREVIEW);

  return (
    <div id="view-home" className="view hm" ref={rootRef}>
      <section className={`hm-hero${urgent ? ' is-urgent' : ''}`} aria-labelledby="hmTitle">
        <p className="hm-eyebrow" data-reveal>{todayLabel()}</p>
        <h1 id="hmTitle" data-reveal data-delay={70}>
          <span className="hm-line">{name === '내' ? '내 작업실.' : <>{name}님의</>}</span>
          {name !== '내' && <span className="hm-line">작업실.</span>}
          {loading ? <span className="hm-line"><SkeletonBlock height={44} className="hm-skel" /></span> : <span className="hm-line hm-accent">{accent}</span>}
        </h1>
        {!loading && <p className="hm-desc" data-reveal data-delay={140}>{desc}</p>}
        {!loading && (
          <div className="hm-actions" data-reveal data-delay={200}>
            <Link className="hm-button" to={cta.to} onPointerEnter={() => prefetchRoute(cta.to)}>{cta.label}<span>{ARROW}</span></Link>
            {urgent && <Link className="hm-link" to="/upload" onPointerEnter={() => prefetchRoute('/upload')}><Glyph name="plus" size={14} />새 발매</Link>}
          </div>
        )}
        {urgent && <p className="hm-pill is-warn" data-reveal data-delay={260}>먼저 할 일</p>}
      </section>

      {!loading && upcoming && <Journey release={upcoming} />}

      {!loading && <MoneyPanel payable={payable} pending={payoutPending} held={held} minimum={minimum} registered={isPaymentRegistered(payment)} />}

      <section className="hm-section" aria-labelledby="hmTasks">
        <Head eyebrow="확인할 작업" title={<span id="hmTasks">{loading ? '불러오고 있어요' : restTasks.length ? `할 일 ${restTasks.length}가지` : '지금은 할 일이 없어요'}</span>}
          sub={!loading && !restTasks.length ? (urgent ? '위의 먼저 할 일만 마치면 돼요.' : '새 일이 생기면 여기에 모아 둘게요.') : undefined} />
        {loading ? <SkeletonRows count={2} /> : restTasks.length > 0 && (
          <>
            <ol className="hm-list">
              {shownTasks.map((t, i) => (
                <li key={t.name} data-reveal data-delay={(i % 4) * 60} data-expand={i >= TASK_PREVIEW || undefined}>
                  <button type="button" className={`hm-row is-${t.tone}`} onClick={() => navigate(t.to)}>
                    <span className="hm-no">{String(i + 1).padStart(2, '0')}</span>
                    <span className="min-0"><strong>{t.name}</strong><small>{t.sub}</small></span>
                    <span className="hm-go">{t.btn}<Glyph name="arrow-right" size={14} /></span>
                  </button>
                </li>
              ))}
            </ol>
            {restTasks.length > TASK_PREVIEW && (
              <button type="button" className="hm-more" aria-expanded={allTasks} onClick={() => setAllTasks(v => !v)}>
                {allTasks ? '접기' : `${restTasks.length - TASK_PREVIEW}가지 더 보기`}
                <Glyph name="chevron-down" size={14} className={allTasks ? 'is-flip' : ''} />
              </button>
            )}
          </>
        )}
      </section>

      <section className="hm-section" aria-label="주요 업무">
        <Head eyebrow="바로가기" title="필요한 곳으로 바로." />
        <div className="hm-services">
          {GRID_ITEMS.map((item, i) => {
            const st = gridStatus[item.key];
            return (
              <Link key={item.to} className="hm-service" to={item.to} onPointerEnter={() => prefetchRoute(item.to)} onFocus={() => prefetchRoute(item.to)} data-reveal data-delay={(i % 2) * 90}>
                <span className="hm-service-top">
                  <span className="hm-no">{String(i + 1).padStart(2, '0')}</span>
                  {st && <span className={`hm-pill${st.tone ? ` is-${st.tone}` : ''}`}>{st.text}</span>}
                </span>
                <strong>{item.title}</strong>
                <small>{item.desc}</small>
              </Link>
            );
          })}
        </div>
      </section>

      <section className="hm-section" aria-labelledby="hmReleases">
        <Head eyebrow="내 발매" title={<span id="hmReleases">{releases.length ? `발매 ${releases.length}건` : '첫 발매를 기다려요'}</span>} link={releases.length ? { to: '/releases', label: '전체 보기' } : undefined} />
        <div className="hm-panel" aria-live="polite" data-reveal>
          {loading ? (
            <SkeletonRows count={3} />
          ) : error ? (
            <div className="empty-note">
              {error}<br />
              <button className="ui-btn is-white is-sm" type="button" onClick={reload}>다시 불러오기</button>
            </div>
          ) : releases.length ? (
            <div className="ui-list">
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
              아직 등록한 발매가 없어요.<br />새 발매를 만들면 여기에서 확인할 수 있어요.
            </div>
          )}
        </div>
      </section>

      <section className="hm-section" aria-labelledby="hmReport">
        <Head eyebrow={`${periodLabel(report.period)} 음악 리포트`} title={<span id="hmReport">{reportEmpty ? '아직 수익 리포트가 없어요' : <Money value={report.revenue} animate />}</span>}
          sub={reportEmpty ? '발매가 플랫폼에 나가고 정산이 들어오면 여기서 수익과 재생 수를 보여 드려요.'
            : report.change != null ? `전월 대비 ${report.change >= 0 ? '+' : ''}${report.change.toFixed(1)}%` : undefined}
          link={{ to: '/reports', label: '리포트 보기' }} />
        {!reportEmpty && (
          <div className="hm-panel hm-report" data-reveal>
            <MiniTrend data={report.trend} />
            <dl className="hm-stats">
              <div><dt>총 재생</dt><dd>{report.plays > 0 ? <CountUp value={report.plays} format={n => `${num(n)}회`} /> : '아직 없어요'}</dd></div>
              <div><dt>Top 트랙</dt><dd>{report.topTrack || '—'}</dd></div>
              <div><dt>Top 플랫폼</dt><dd>{report.topPlatform || '—'}</dd></div>
            </dl>
          </div>
        )}
      </section>
    </div>
  );
}
