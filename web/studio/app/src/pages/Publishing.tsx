// 퍼블리싱 메뉴 — 보고 분석하는 곳: 분석(할 일·리포트·현황·곡별) · 곡 · 저작권료(자세한 내역).
// 앨범을 골라 그 앨범만 볼 수 있다 (?release=). 신청·곡 정리·계약 같은 '관리'는 발매 상세 '퍼블리싱' 탭에서 하고,
// 그 탭에는 번 돈을 한 줄로만 보여 주고 자세한 건 여기로 잇는다. Open A&R은 홍보 메뉴, 계약서·정산·알림·문의·계정은 배급과 함께 쓴다
import { useDeferredValue, useMemo, useState } from 'react';
import { Link, Navigate, useNavigate, useSearchParams } from '../lib/router';
import { pubApi, type PublishingSummary, type PublishingTask, type Royalty, type Work } from '../api/publishing';
import { useAsync } from '../hooks/useAsync';
import { APP_LABEL, APP_TONE, matchFilter, type WorkFilter } from '../lib/publishing';
import { Glyph, type GlyphName } from '../components/Glyph';
import { ReleaseCover } from '../components/ReleaseCover';
import { SkeletonRows } from '../components/Skeleton';
import { Money } from '../components/Money';
import { RegBadge } from '../components/publishing/parts';
import { RoyaltyTable } from '../components/publishing/Royalties';
import { MiniTrend } from '../components/MiniTrend';
import { publishingReport } from '../lib/publishingReport';
import { periodLabel } from '../data/reports';
import { money, num } from '../lib/format';
import { Badge, EmptyState, IconTile, KeyValue, Meter, StatGrid, StatTile, Tabs } from '../ui';
import { MonthField } from '../components/Picker';
import {
  ROYALTY_PERIODS, filterFromParams, filterRoyalties, filterToParams, isDefaultFilter, royaltyFilterLabel, type RoyaltyFilter, type RoyaltyPeriod,
} from '../lib/publishingPrep';
import '../styles/split/Dashboard.css';
import '../styles/split/Reports.css';
import '../styles/split/Publishing.css';

type Tab = 'overview' | 'songs' | 'royalties';
const TABS: { value: Tab; label: string }[] = [{ value: 'overview', label: '분석' }, { value: 'songs', label: '곡' }, { value: 'royalties', label: '저작권료' }];
/** 앨범 하나만 볼 때 — 그 앨범에 실린 곡과 그 곡들의 저작권료만 */
interface Scope { releaseId: string; title: string }
const inScope = (w: Work, scope: Scope | null) => !scope || w.recordings.some(r => r.releaseId === scope.releaseId);
const SONG_FILTERS: [WorkFilter, string][] = [['all', '전체'], ['managed', '관리 중'], ['review', '심사 중'], ['fix', '보완 필요'], ['none', '미신청'], ['unreleased', '미발매']];
const TASK_ICON: Record<PublishingTask['kind'], GlyphName> = { fix: 'warning', split: 'users', sign: 'sign', renew: 'repeat', ar: 'megaphone' };
const TASK_BTN: Record<PublishingTask['kind'], string> = { fix: '보완', split: '확인', sign: '서명', renew: '갱신', ar: '보기' };
const shortWon = (n: number) => (n >= 10000 ? `${(n / 10000).toFixed(n >= 100000 ? 0 : 1)}만` : num(n));
const PATH = [
  ['저작권 관리 신청', '맡길 곡과 지분을 고르고 신청서에 서명'],
  ['담당자 심사', '참여자·지분·권리를 확인해요'],
  ['계약서 서명', '수수료·관리 범위를 확인하고 서명'],
  ['관리 단체 등록', 'KOMCA 등 관리 단체에 곡을 등록해요'],
  ['저작권료 정산', '들어온 저작권료는 배급 정산과 같은 잔액으로'],
] as const;
const norm = (s: string) => s.toLowerCase().replace(/\s+/g, '');
// 손이 가야 하는 곡이 먼저 — 보완 요청 → 맡기지 않음 → 심사 중 → 관리 중
const urgency = (w: Work) => (matchFilter(w, 'fix') ? 0 : matchFilter(w, 'none') ? 1 : matchFilter(w, 'review') ? 2 : 3);

function SongRow({ w, releaseId }: { w: Work; releaseId?: string }) {
  const rec = w.recordings[0];
  return (
    <Link to={`/works/${w.id}${releaseId ? `?release=${encodeURIComponent(releaseId)}` : ''}`} className="track-row ui-row aq-track-row" aria-label={`${w.title || '제목 없는 곡'} 저작권 보기`}>
      <ReleaseCover id={rec?.releaseId ?? w.id} />
      <div className="min-0">
        <span className="ui-row-title">{w.title || '제목 없는 곡'}</span>
        <span className="ui-row-meta">{rec ? rec.releaseTitle : '미발매'} · {w.contributors.map(c => c.name).join(' · ') || '참여자 없음'}</span>
      </div>
      {w.application && !['approved', 'withdrawn'].includes(w.application.status)
        ? <Badge tone={APP_TONE[w.application.status]}>{APP_LABEL[w.application.status]}</Badge>
        : <RegBadge status={w.registration} />}
    </Link>
  );
}

/** 분석 — 확인할 일 · 저작권료 리포트 · 관리 중인 곡 · 곡별 저작권료 */
function Overview({ scope, works, rows, label }: { scope: Scope | null; works: Work[] | null; rows: Royalty[] | null; label: string }) {
  const nav = useNavigate();
  const { data: sum, loading } = useAsync(() => pubApi.summary(), [], { key: 'pub-summary' });
  if (loading && !sum) return <SkeletonRows count={3} />;
  if (!sum || !works) return null;
  // 관리 현황은 선택한 앨범을 기준으로 센다
  const mine = works.filter(w => inScope(w, scope));
  const s = scope ? {
    ...sum, works: {
      total: mine.length, managed: mine.filter(w => matchFilter(w, 'managed')).length,
      inProgress: mine.filter(w => matchFilter(w, 'review') || matchFilter(w, 'fix')).length,
      unregistered: mine.filter(w => matchFilter(w, 'none')).length,
    },
  } : sum;
  return (
    <div className="aq-detail-stack">
      {!s.works.total ? (
        <EmptyState icon={<Glyph name="scale" size={22} />} title={scope ? '이 앨범에는 아직 등록한 곡이 없어요' : '아직 등록한 곡이 없어요'}
          actions={<Link to={scope ? `/releases/${encodeURIComponent(scope.releaseId)}?tab=publishing` : '/publishing/apply'} className="ui-btn is-primary">{scope ? '앨범에서 시작하기' : '저작권 관리 신청'}</Link>}>
          곡을 쓴 사람에게는 작사·작곡 저작권이 있어요. 맡기면 방송·스트리밍 저작권료를 AUDENIQ이 받아 배급 정산과 같은 잔액으로 드려요.
        </EmptyState>
      ) : (
        <>
          {!scope && <Tasks s={s} nav={nav} />}
          <div className="aq-pbl-dashboard">
            <RoyaltyReport rows={rows} label={label} />
            <section className="ui-card aq-pbl-status" aria-labelledby="pubHeroSongs">
              <div className="ui-section-head"><h2 id="pubHeroSongs">곡 관리 현황</h2><span className="ui-count is-soft">전체 {s.works.total}곡</span></div>
              <StatGrid cols={3}>
                <StatTile compact label="관리 중" value={s.works.managed} unit="곡" />
                <StatTile compact label="진행 중" value={s.works.inProgress} unit="곡" />
                <StatTile compact label="미신청" value={s.works.unregistered} unit="곡" />
              </StatGrid>
              <Link to={`/publishing?tab=songs${scope ? `&release=${encodeURIComponent(scope.releaseId)}` : ''}`} className="aq-head-link">곡별 상태 보기<Glyph name="chevron-right" size={14} /></Link>
              {scope && <Link to={`/releases/${encodeURIComponent(scope.releaseId)}?tab=publishing`} className="ui-btn is-soft is-block">이 앨범에서 관리하기<Glyph name="chevron-right" size={14} /></Link>}
            </section>
          </div>
          <SongRoyalties rows={rows} works={works} label={label} />
          {/* 아직 맡긴 곡이 없을 때만 — 처음 쓰는 사람에게 순서를 */}
          {!scope && !s.works.managed && <PathGuide />}
        </>
      )}
    </div>
  );
}

/** 확인할 일 — 홈 '확인할 작업'과 같은 줄: 아이콘 · 할 일 · 오른쪽 버튼 글자 */
function Tasks({ s, nav }: { s: PublishingSummary; nav: (to: string) => void }) {
  if (!s.tasks.length && !s.suggestion) return null;
  return (
    <section className="ui-card aq-home-tasks" aria-labelledby="pubTasks">
      <div className="ui-section-head">
        <h2 id="pubTasks">확인할 일</h2>
        <span className="ui-count is-soft">{s.tasks.length + (s.suggestion ? 1 : 0)}</span>
      </div>
      <ul className="aq-task-list aq-stagger">
        {s.tasks.map(t => (
          <li key={t.id}>
            <button type="button" className={`aq-task is-${t.kind === 'fix' ? 'warn' : 'info'}`} onClick={() => nav(t.to)}>
              <span className="ui-icon" aria-hidden="true"><Glyph name={TASK_ICON[t.kind]} size={20} /></span>
              <span className="aq-task-text"><strong>{t.title}</strong><small>{t.sub}</small></span>
              <span className="aq-task-go">{TASK_BTN[t.kind]}<i aria-hidden="true"><Glyph name="chevron-right" size={14} /></i></span>
            </button>
          </li>
        ))}
        {s.suggestion && (
          <li>
            <button type="button" className="aq-task is-info" onClick={() => nav(`/works/new?release=${encodeURIComponent(s.suggestion!.releaseId)}&track=${encodeURIComponent(s.suggestion!.trackId)}`)}>
              <span className="ui-icon" aria-hidden="true"><Glyph name="plus" size={20} /></span>
              <span className="aq-task-text"><strong>‘{s.suggestion.title}’ 저작권도 맡길까요?</strong><small>{s.suggestion.releaseTitle}에 실린 곡이에요</small></span>
              <span className="aq-task-go">정리<i aria-hidden="true"><Glyph name="chevron-right" size={14} /></i></span>
            </button>
          </li>
        )}
      </ul>
    </section>
  );
}

/** 퍼블리싱은 이렇게 진행돼요 — 처음 쓰는 사람을 위한 순서 */
function PathGuide() {
  return (
      <section className="aq-doc-path-wrap" aria-labelledby="pubPath">
        <h2 id="pubPath">퍼블리싱은 이렇게 진행돼요</h2>
        <ol className="ui-card ui-list is-inset aq-doc-path">
          {PATH.map(([title, when], i) => (
            <li key={title} className="ui-row">
              <IconTile size="sm"><span className="aq-preview-no">{i + 1}</span></IconTile>
              <span className="ui-row-body min-0"><b className="ui-row-title">{title}</b><span className="ui-row-meta">{when}</span></span>
            </li>
          ))}
        </ol>
      </section>
  );
}

function Songs({ filter, setFilter, scope }: { filter: WorkFilter; setFilter: (f: WorkFilter) => void; scope: Scope | null }) {
  const nav = useNavigate();
  const { data: all, loading } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  const works = useMemo(() => all && all.filter(w => inScope(w, scope)), [all, scope]);
  const [query, setQuery] = useState('');
  const q = norm(useDeferredValue(query));
  const shown = useMemo(() => (works ?? []).filter(w => matchFilter(w, filter)
    && (!q || norm(w.title).includes(q) || w.contributors.some(c => norm(c.name).includes(q)) || w.recordings.some(r => norm(r.releaseTitle).includes(q))))
    .sort((a, b) => urgency(a) - urgency(b)), [works, filter, q]);
  return (
    <>
      <div className="toolbar">
        <input id="pubSongSearch" className="ui-input" type="search" aria-label="곡 검색" placeholder="곡·참여자·발매 검색" value={query} onChange={e => setQuery(e.target.value)} />
      </div>
      {/* 발매 목록과 같은 칩 — 숫자를 붙이고, 해당하는 곡이 없는 칩은 숨긴다 */}
      <div className="ui-chips" role="group" aria-label="저작권 상태">
        {SONG_FILTERS.map(([v, label]) => {
          const n = (works ?? []).filter(w => matchFilter(w, v)).length;
          if (v !== 'all' && !n && filter !== v) return null;
          return <button key={v} type="button" className={`ui-chip${filter === v ? ' is-on' : ''}`} aria-pressed={filter === v} onClick={() => setFilter(v)}>{label}<small>{n}</small></button>;
        })}
      </div>
      <div className="aq-pub-count">
        <span className="studio-result-count" aria-live="polite">{works ? `${shown.length}개의 곡` : ''}</span>
        {/* 발매 전 곡도 작사·작곡 정보와 지분을 먼저 정리해 둘 수 있다 */}
        <button type="button" className="ui-btn is-ghost is-sm" onClick={() => nav('/works/new')}><Glyph name="plus" size={14} className="aq-glyph-lead" />곡 추가</button>
      </div>
      {loading && !works ? <SkeletonRows count={4} /> : shown.length ? (
        <div className="aq-catalog-cards aq-stagger">{shown.map(w => <SongRow key={w.id} w={w} releaseId={scope?.releaseId} />)}</div>
      ) : (
        <div className="ui-empty">
          <h2>{query || filter !== 'all' ? '조건에 맞는 곡이 없어요.' : '아직 곡이 없어요.'}</h2>
          <p>{query ? '다른 검색어로 찾아보세요.' : '발매를 접수하면 적은 크레딧으로 곡이 여기에 정리돼요. 발매 전 곡은 직접 추가할 수 있어요.'}</p>
        </div>
      )}
    </>
  );
}

/** 저작권료 리포트 — 홈 '음악 리포트'와 같은 카드: 최근 달 금액 · 지난달 비교 · 월별 막대 · 많이 번 곡과 단체 */
function RoyaltyReport({ rows, label }: { rows: Royalty[] | null; label: string }) {
  const nav = useNavigate();
  const [params] = useSearchParams();
  const r = useMemo(() => publishingReport(rows ?? []), [rows]);
  return (
    <section className="ui-card aq-report-card" aria-labelledby="pubReport">
      <div className="ui-section-head aq-report-head">
        <h2 id="pubReport">저작권료 리포트{r && <small>{periodLabel(r.period)}</small>}</h2>
        {/* 고른 기간·조건을 그대로 들고 내역으로 */}
        <button className="aq-head-link" type="button" onClick={() => { const p = new URLSearchParams(params); p.set('tab', 'royalties'); p.delete('wf'); nav(`/publishing?${p}`); }}>내역 보기<Glyph name="chevron-right" size={14} /></button>
      </div>
      {!rows ? <SkeletonRows count={2} pattern="amount" /> : !r ? (
        <EmptyState compact icon={<Glyph name="chart" size={22} />} title={label ? '이 조건에 맞는 저작권료가 없어요' : '아직 들어온 저작권료가 없어요'}>
          {label ? `${label} 기준으로 들어온 내역이 없어요. 기간을 넓히거나 조건을 바꿔 보세요.` : '관리 단체가 사용료를 나눠 주면 여기서 달마다 얼마나 벌었는지 보여 드려요. 보통 사용된 달부터 3~6개월 걸려요.'}
        </EmptyState>
      ) : (<>
        <div className="aq-report-total">
          <small>{r.hasConfirmed ? `최근 확정 · ${Number(r.period.slice(5))}월분 저작권료` : '확정 전 예상 내역만 있어요'}</small>
          <strong className={!r.hasConfirmed ? 'aq-pbl-report-pending' : undefined}>{r.hasConfirmed ? <Money value={r.amount} animate /> : '확정 내역을 기다리고 있어요'}</strong>
          {(r.estimated || r.prev != null) && (
            <div className="aq-home-report-note">
              {r.estimated > 0 && <small>확정 전 예상 <Money value={r.estimated} /></small>}
              {r.prev != null && <small>지난달 {money(r.prev)}</small>}
            </div>
          )}
        </div>
        {r.hasConfirmed && <MiniTrend data={r.trend} label="최근 월별 확정 저작권료 추이" short={shortWon} format={money} />}
        <KeyValue className="aq-report-kv" cols={2} items={[
          ['이달의 주요 곡', <span className="aq-report-top"><IconTile size="sm"><Glyph name="music" size={16} /></IconTile><span>{r.topSong || '—'}</span></span>],
          ['이달의 주요 징수 단체', <span className="aq-report-top"><IconTile size="sm"><Glyph name="globe" size={16} /></IconTile><span>{r.topSource || '—'}</span></span>],
        ]} />
        <p className="ui-row-meta aq-report-foot">관리 단체가 확정한 저작권료는 <Link to="/settlement?type=publishing" className="aq-inline-link">정산·지급</Link> 잔액에 배급 정산금과 함께 쌓여요.</p>
      </>)}
    </section>
  );
}

/** 곡별 저작권료 — 홈 '내 발매'와 같은 줄에 몫 막대만 더했다. 어느 곡이 가장 많이 벌었는지 (예상 제외) */
function SongRoyalties({ rows, works, label }: { rows: Royalty[] | null; works: Work[] | null; label: string }) {
  const [expanded, setExpanded] = useState(false);
  const bySong = new Map<string, { title: string; id: string; net: number; count: number }>();
  for (const r of rows ?? []) {
    if (r.status === 'estimated') continue;
    const cur = bySong.get(r.workId) ?? { title: r.workTitle, id: r.workId, net: 0, count: 0 };
    cur.net += r.net; cur.count += 1; bySong.set(r.workId, cur);
  }
  const top = [...bySong.values()].sort((a, b) => b.net - a.net);
  const max = top[0]?.net ?? 0;
  if (!top.length) return null;
  const rec = (id: string) => works?.find(w => w.id === id)?.recordings[0];
  return (
    <section className="ui-card aq-pub-songs" aria-labelledby="pubBySong">
      <div className="ui-section-head"><h2 id="pubBySong">곡별 저작권료</h2><span className="ui-row-meta">{label ? `${label} · 확정 금액` : '누적 확정 금액'}</span></div>
      <div className="ui-list aq-stagger">
        {(expanded ? top : top.slice(0, 5)).map(s => (
          <Link key={s.id} to={`/works/${s.id}?tab=revenue`} className="release-row ui-row studio-album-row aq-row-link">
            <ReleaseCover id={rec(s.id)?.releaseId ?? s.id} />
            <div className="min-0">
              <span className="ui-row-title">{s.title}</span>
              <span className="ui-row-meta">{rec(s.id)?.releaseTitle ?? '미발매'} · 확정 {s.count}건</span>
              <Meter parts={[{ value: s.net }]} total={max} label={`${s.title} 저작권료`} />
            </div>
            <div className="ui-row-end"><b className="aq-pub-amount"><Money value={s.net} /></b></div>
          </Link>
        ))}
      </div>
      {top.length > 5 && <button type="button" className="ui-btn is-soft is-block" aria-expanded={expanded} onClick={() => setExpanded(v => !v)}>{expanded ? '접기' : `전체 ${top.length}곡 보기`}</button>}
    </section>
  );
}

/** 저작권료 — 기간별 자세한 내역과 CSV (합계·추이는 '분석' 탭) */
function RoyaltiesTab({ rows, label }: { rows: Royalty[] | null; label: string }) {
  if (!rows) return <SkeletonRows count={3} pattern="amount" />;
  return (
    <>
      <p className="ui-field-help">확정된 저작권료는 <Link to="/settlement?type=publishing" className="aq-inline-link">정산·지급</Link> 잔액에 배급 정산금과 함께 쌓여요.</p>
      <RoyaltyTable summary={false} rows={rows} csvLabel={label}
        emptyText={label ? `${label} 기준으로 들어온 내역이 없어요. 기간을 넓히거나 조건을 바꿔 보세요.` : '관리 단체가 사용료를 나눠 주면 여기서 곡별로 보여 드려요. 보통 사용된 달부터 3~6개월 걸려요.'} />
    </>
  );
}

/** 분석 기간·조건 — 음악 리포트와 같은 줄(기간 고르기 + 직접 선택 달 칸 + 고르기 칸). 고른 조건은 주소에 남아
 *  분석 · 저작권료 내역 · CSV가 같은 조건을 쓴다 */
function RoyaltyFilterBar({ value, onChange, rows, works }: { value: RoyaltyFilter; onChange: (f: RoyaltyFilter) => void; rows: Royalty[]; works: Work[] }) {
  const songs = useMemo(() => {
    const m = new Map<string, string>();
    for (const r of rows) if (!m.has(r.workId)) m.set(r.workId, works.find(w => w.id === r.workId)?.title ?? r.workTitle);
    return [...m.entries()].sort((a, b) => a[1].localeCompare(b[1], 'ko'));
  }, [rows, works]);
  const sources = useMemo(() => [...new Set(rows.map(r => r.source))].sort(), [rows]);
  const countries = useMemo(() => [...new Set(rows.map(r => r.country))].sort(), [rows]);
  const set = (patch: Partial<RoyaltyFilter>) => onChange({ ...value, ...patch });
  const bad = value.period === 'custom' && !!value.from && !!value.to && value.from > value.to;
  const thisMonth = new Date().toISOString().slice(0, 7);
  return (
    <>
      <div className="report-tools aq-pbl-filter">
        <select className="ui-select" aria-label="분석 기간" value={value.period} onChange={e => set({ period: e.target.value as RoyaltyPeriod })}>
          {ROYALTY_PERIODS.map(p => <option key={p.value} value={p.value}>{p.label}</option>)}
        </select>
        {value.period === 'custom' && (<>
          <MonthField id="pubFrom" aria-label="시작 월" value={value.from ?? ''} max={value.to || thisMonth} onChange={v => set({ from: v })} />
          <MonthField id="pubTo" aria-label="끝 월" value={value.to ?? ''} min={value.from || undefined} max={thisMonth} onChange={v => set({ to: v })} />
        </>)}
        <select className="ui-select" aria-label="곡" value={value.work ?? ''} onChange={e => set({ work: e.target.value || undefined })}>
          <option value="">모든 곡</option>
          {songs.map(([id, t]) => <option key={id} value={id}>{t}</option>)}
        </select>
        <select className="ui-select" aria-label="징수 단체" value={value.source ?? ''} onChange={e => set({ source: e.target.value || undefined })}>
          <option value="">모든 징수 단체</option>
          {sources.map(x => <option key={x} value={x}>{x}</option>)}
        </select>
        <select className="ui-select" aria-label="국가" value={value.country ?? ''} onChange={e => set({ country: e.target.value || undefined })}>
          <option value="">모든 국가</option>
          {countries.map(x => <option key={x} value={x}>{x}</option>)}
        </select>
        {!isDefaultFilter(value) && <button type="button" className="ui-btn is-text is-sm" onClick={() => onChange({ period: '12m' })}>조건 지우기</button>}
      </div>
      {bad && <p className="ui-field-help aq-help-error" role="alert">시작 월이 끝 월보다 늦어요. 기간을 다시 골라 주세요.</p>}
    </>
  );
}

/** 앨범 고르기 — 전체를 보다가 앨범 하나만 좁혀 본다. 발매 상세 '퍼블리싱' 탭의 '자세히 보기'도 이 앨범을 골라 둔 채로 들어온다 */
function AlbumScope({ works, value, onChange }: { works: Work[] | null; value: string | null; onChange: (id: string | null) => void }) {
  const albums = useMemo(() => {
    const m = new Map<string, string>();
    for (const w of works ?? []) for (const r of w.recordings) if (!m.has(r.releaseId)) m.set(r.releaseId, r.releaseTitle);
    return [...m.entries()];
  }, [works]);
  if (albums.length < 2 && !value) return null;
  return (
    <div className="aq-pub-scope">
      <select className="ui-select" aria-label="볼 앨범" value={value ?? ''} onChange={e => onChange(e.target.value || null)}>
        <option value="">모든 앨범</option>
        {albums.map(([id, title]) => <option key={id} value={id}>{title}</option>)}
      </select>
      {value && <Link to={`/releases/${encodeURIComponent(value)}?tab=publishing`} className="ui-btn is-ghost is-sm">앨범에서 관리<Glyph name="chevron-right" size={14} /></Link>}
    </div>
  );
}

export function Publishing() {
  const nav = useNavigate();
  const [params, setParams] = useSearchParams();
  const asked = params.get('tab') as Tab | null;
  const tab: Tab = asked && TABS.some(t => t.value === asked) ? asked : 'overview';
  const songFilter = (params.get('wf') as WorkFilter | null) ?? 'all';
  const set = (patch: Record<string, string | null>) => {
    const p = new URLSearchParams(params);
    for (const [k, v] of Object.entries(patch)) { if (v === null) p.delete(k); else p.set(k, v); }
    setParams(p, { replace: true });
  };
  const go = (t: Tab) => set({ tab: t === 'overview' ? null : t, wf: null });  // 고른 앨범(?release)은 탭을 옮겨도 그대로
  const { data: works } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  const { data: royalties } = useAsync(() => pubApi.listRoyalties(), [], { key: 'royalties:all' });
  const releaseId = params.get('release');
  const scopeTitle = releaseId ? works?.flatMap(w => w.recordings).find(r => r.releaseId === releaseId)?.releaseTitle : undefined;
  const scope: Scope | null = releaseId ? { releaseId, title: scopeTitle ?? '' } : null;
  const scopedRows = useMemo(() => {
    if (!royalties || !works) return null;
    if (!releaseId) return royalties;
    const ids = new Set(works.filter(w => w.recordings.some(r => r.releaseId === releaseId)).map(w => w.id));
    return royalties.filter(r => ids.has(r.workId));
  }, [royalties, works, releaseId]);
  // 분석 기간·조건 — 분석 탭과 저작권료 탭(내역·CSV)이 같은 조건을 쓴다
  const filter = useMemo(() => filterFromParams(params), [params]);
  const shownRows = useMemo(() => (scopedRows ? filterRoyalties(scopedRows, filter) : null), [scopedRows, filter]);
  // 아직 저작권료가 하나도 없으면 조건 이야기는 하지 않는다 ('최근 1년 기준 없음'이 아니라 '아직 없음')
  const filterLabel = scopedRows?.length ? royaltyFilterLabel(filter, { work: works?.find(w => w.id === filter.work)?.title }) : '';
  // 예전 탭 주소는 기능이 옮겨 간 화면으로 — 계약서, Open A&R(홍보)
  const moved = asked === 'contracts' as string ? `/contracts${params.get('pub') ? `?pub=${encodeURIComponent(params.get('pub')!)}` : ''}`
    : asked === 'ar' as string ? `/ar${params.get('work') ? `?work=${encodeURIComponent(params.get('work')!)}` : ''}`
    : null;
  if (moved) return <Navigate to={moved} replace />;

  return (
    <div className="view" id="view-publishing">
      <div className="ui-page-head aq-head-inline">
        <div>
          <p className="eyebrow">작업실</p>
          <h1>퍼블리싱</h1>
          <p>곡의 작사·작곡 저작권을 등록하고 저작권료를 받아요.</p>
        </div>
        <button type="button" className="ui-btn is-primary" onClick={() => nav(`/publishing/apply${releaseId ? `?release=${encodeURIComponent(releaseId)}` : ''}`)}><Glyph name="plus" size={14} />저작권 관리 신청</button>
      </div>

      <Tabs label="퍼블리싱 메뉴" value={tab} onChange={go}
        items={TABS.map(t => ({ ...t, count: t.value === 'songs' ? works?.filter(w => inScope(w, scope)).length ?? null : undefined }))} />

      <AlbumScope works={works ?? null} value={releaseId} onChange={id => set({ release: id, work: null })} />
      {tab !== 'songs' && scopedRows && scopedRows.length > 0 && (
        <RoyaltyFilterBar value={filter} rows={scopedRows} works={works ?? []} onChange={f => set(filterToParams(f))} />
      )}

      <div className="ui-tab-panel" key={tab}>
        {tab === 'overview' && <Overview scope={scope} works={works ?? null} rows={shownRows} label={filterLabel} />}
        {tab === 'songs' && <Songs scope={scope} filter={songFilter} setFilter={f => set({ wf: f === 'all' ? null : f })} />}
        {tab === 'royalties' && <RoyaltiesTab rows={shownRows} label={filterLabel} />}
      </div>
    </div>
  );
}
