// 퍼블리싱은 따로 메뉴를 두지 않고 배급 화면에 녹인다 — 이 파일은 그 조각들:
//  · PublishingSongs: '발매·곡 관리' › '곡 저작권' 탭 (발매한 곡 + 아직 발매하지 않은 곡)
//  · PublishingReport: '음악 리포트' › '저작권료' (기간·곡·징수 단체·국가 조건, 리포트 카드, 곡별 금액, 내역·CSV)
//  · Publishing: 예전 /publishing 주소를 새 자리로 보낸다
// 신청·곡 정리·계약 같은 '관리'는 발매 상세 '퍼블리싱' 탭에서. 할 일은 홈 '확인할 작업', 계약서·정산·알림·문의·계정은 배급과 함께 쓴다
import { useDeferredValue, useMemo, useState } from 'react';
import { Link, Navigate, useNavigate, useSearchParams } from '../lib/router';
import { pubApi, type ArSubmission, type Royalty, type Work } from '../api/publishing';
import { useAsync } from '../hooks/useAsync';
import { APP_LABEL, APP_TONE, AR_LABEL, AR_TONE, PAYOUT_STEPS, PAYOUT_TIMING, matchFilter, missedRoyalties, workOrigin, type WorkFilter } from '../lib/publishing';
import { api } from '../api/client';
import { useReportRows } from '../hooks/useReportRows';
import { Glyph } from '../components/Glyph';
import { ReleaseCover } from '../components/ReleaseCover';
import { SkeletonRows } from '../components/Skeleton';
import { Money } from '../components/Money';
import { RegBadge } from '../components/publishing/parts';
import { RoyaltyTable } from '../components/publishing/Royalties';
import { MiniTrend } from '../components/MiniTrend';
import { publishingReport } from '../lib/publishingReport';
import { periodLabel } from '../data/reports';
import { money, num } from '../lib/format';
import { Badge, ChipGroup, EmptyState, IconTile, KeyValue, Meter } from '../ui';
import { MonthField } from '../components/Picker';
import {
  ROYALTY_PERIODS, filterFromParams, filterRoyalties, filterToParams, isDefaultFilter, royaltyFilterLabel, type RoyaltyFilter, type RoyaltyPeriod,
} from '../lib/publishingPrep';
import '../styles/split/Dashboard.css';
import '../styles/split/Reports.css';
import '../styles/split/Publishing.css';

/** 앨범 하나만 볼 때 — 그 앨범에 실린 곡과 그 곡들의 저작권료만 */
interface Scope { releaseId: string; title: string }
const inScope = (w: Work, scope: Scope | null) => !scope || w.recordings.some(r => r.releaseId === scope.releaseId);
const SONG_FILTERS: [WorkFilter, string][] = [['all', '전체'], ['managed', '관리 중'], ['review', '심사 중'], ['fix', '보완 필요'], ['none', '미신청'], ['unreleased', '발매 전']];
const shortWon = (n: number) => (n >= 10000 ? `${(n / 10000).toFixed(n >= 100000 ? 0 : 1)}만` : num(n));
const norm = (s: string) => s.toLowerCase().replace(/\s+/g, '');
// 손이 가야 하는 곡이 먼저 — 보완 요청 → 맡기지 않음 → 심사 중 → 관리 중
const urgency = (w: Work) => (matchFilter(w, 'fix') ? 0 : matchFilter(w, 'none') ? 1 : matchFilter(w, 'review') ? 2 : 3);

/** 곡이 실린 발매 한 줄 — 내 발매는 발매 제목, 다른 아티스트 발매는 '이온 발매', 아니면 발매 전 (목록·리포트가 같은 말) */
const releaseLine = (w?: Work) => !w ? '발매 정보 없음' : w.recordings[0]?.releaseTitle
  ?? (workOrigin(w) === 'other_release' ? `${w.externalRelease?.artist ?? '다른 아티스트'} 발매` : '발매 전');

function SongRow({ w, releaseId, ar }: { w: Work; releaseId?: string; ar?: ArSubmission }) {
  const rec = w.recordings[0];
  return (
    <Link to={`/works/${w.id}${releaseId ? `?release=${encodeURIComponent(releaseId)}` : ''}`} className="track-row ui-row aq-track-row" aria-label={`${w.title || '제목 없는 곡'} 저작권 보기`}>
      <ReleaseCover id={rec?.releaseId ?? w.id} />
      <div className="min-0">
        <span className="ui-row-title">{w.title || '제목 없는 곡'}</span>
        <span className="ui-row-meta">{releaseLine(w)} · {w.contributors.map(c => c.name).join(' · ') || '참여자 없음'}</span>
      </div>
      {/* 발매 전 곡은 저작권 관리 대상이 아니라 '미신청'이 아니다 — A&R 진행 상태나 '발매 전'으로 */}
      {workOrigin(w) === 'unreleased' && w.registration === 'none'
        ? (ar ? <Badge tone={AR_TONE[ar.status]}>A&R {AR_LABEL[ar.status]}</Badge> : <Badge tone="gray">발매 전</Badge>)
        : w.application && !['approved', 'withdrawn'].includes(w.application.status)
        ? <Badge tone={APP_TONE[w.application.status]}>{APP_LABEL[w.application.status]}</Badge>
        : <RegBadge status={w.registration} />}
    </Link>
  );
}

/** 분석 — 저작권료 리포트 · 곡 관리 현황 · 곡별 저작권료 */
function Overview({ scope, works, rows, label }: { scope: Scope | null; works: Work[] | null; rows: Royalty[] | null; label: string }) {
  if (!works) return <SkeletonRows count={3} />;
  const total = works.filter(w => inScope(w, scope)).length;
  // 리포트는 돈 이야기만 — 곡이 몇 곡 관리 중인지는 '발매·곡 관리 › 곡 저작권'에, 처음 순서는 앨범 '퍼블리싱' 탭에 있다
  return (
    <div className="aq-detail-stack">
      {!total ? (
        <EmptyState icon={<Glyph name="scale" size={22} />} title={scope ? '이 앨범에는 아직 등록한 곡이 없어요' : '아직 등록한 곡이 없어요'}
          actions={<Link to={scope ? `/releases/${encodeURIComponent(scope.releaseId)}?tab=publishing` : '/releases?tab=works'} className="ui-btn is-primary">{scope ? '앨범에서 시작하기' : '곡 저작권 보기'}</Link>}>
          곡을 쓴 사람에게는 작사·작곡 저작권이 있어요. 맡기면 방송·스트리밍 저작권료를 AUDENIQ이 받아 배급 정산과 같은 잔액으로 드려요.
        </EmptyState>
      ) : (
        <>
          <RoyaltyReport rows={rows} label={label} />
          <SongRoyalties rows={rows} works={works} label={label} />
        </>
      )}
    </div>
  );
}

function Songs({ filter, setFilter, scope, onApply }: { filter: WorkFilter; setFilter: (f: WorkFilter) => void; scope: Scope | null; onApply: () => void }) {
  const nav = useNavigate();
  const { data: all, loading } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  const works = useMemo(() => all && all.filter(w => inScope(w, scope)), [all, scope]);
  const { data: arList } = useAsync(() => pubApi.listAr(), [], { key: 'ar-submissions' });
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
      <ChipGroup label="저작권 상태" value={filter} onChange={setFilter} hideZero={false}
        options={SONG_FILTERS.map(([value, label]) => {
          const count = (works ?? []).filter(w => matchFilter(w, value)).length;
          return { value, label, count, hidden: value !== 'all' && !count && filter !== value };
        })} />
      <div className="aq-pub-count">
        <span className="studio-result-count" aria-live="polite">{works ? `${shown.length}개의 곡` : ''}</span>
        {/* 발매 전 곡도 작사·작곡 정보와 지분을 먼저 정리해 둘 수 있다 */}
        <span className="aq-pub-count-actions">
          <button type="button" className="ui-btn is-ghost is-sm" onClick={() => nav('/works/new')}><Glyph name="plus" size={14} className="aq-glyph-lead" />곡 추가</button>
          <button type="button" className="ui-btn is-soft is-sm" onClick={onApply}>저작권 관리 신청</button>
        </span>
      </div>
      {loading && !works ? <SkeletonRows count={4} /> : shown.length ? (
        <div className="aq-catalog-cards aq-stagger">{shown.map(w => <SongRow key={w.id} w={w} releaseId={scope?.releaseId} ar={arList?.find(a => a.workId === w.id && a.status !== 'closed')} />)}</div>
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
  const r = useMemo(() => publishingReport(rows ?? []), [rows]);
  return (
    <section className="ui-card aq-report-card" aria-labelledby="pubReport">
      <div className="ui-section-head aq-report-head">
        <h2 id="pubReport">저작권료 리포트{r && <small>{periodLabel(r.period)}</small>}</h2>
        {/* 고른 기간·조건을 그대로 들고 내역으로 */}
        <button className="aq-head-link" type="button" onClick={() => document.getElementById('pubRoyaltyList')?.scrollIntoView({ behavior: 'smooth', block: 'start' })}>내역 보기<Glyph name="chevron-right" size={14} /></button>
      </div>
      {!rows ? <SkeletonRows count={2} pattern="amount" /> : !r ? (<>
        <EmptyState compact icon={<Glyph name="chart" size={22} />} title={label ? '이 조건에 맞는 저작권료가 없어요' : '아직 들어온 저작권료가 없어요'}>
          {label ? `${label} 기준으로 들어온 내역이 없어요. 기간을 넓히거나 조건을 바꿔 보세요.` : '관리 단체가 사용료를 나눠 주면 여기서 달마다 얼마나 벌었는지 보여 드려요.'}
        </EmptyState>
        {/* 처음 기다리는 사람에게 — 저작권료가 어떤 순서로, 얼마나 걸려 들어오는지 */}
        {!label && <ol className="aq-pbl-payout">{PAYOUT_STEPS.map(([t, d], k) => <li key={t}><span className="aq-bundle-no" aria-hidden="true">{k + 1}</span><span className="min-0"><strong>{t}</strong><small>{d}</small></span></li>)}</ol>}
      </>) : (<>
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
        {/* 잔액 안내는 아래 저작권료 내역에 한 번만 — 같은 문장이 두 번 보였다 */}
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
              <span className="ui-row-meta">{releaseLine(works?.find(w => w.id === s.id))} · 확정 {s.count}건</span>
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
        emptyText={label ? `${label} 기준으로 들어온 내역이 없어요. 기간을 넓히거나 조건을 바꿔 보세요.` : `관리 단체가 사용료를 나눠 주면 여기서 곡별로 보여 드려요. ${PAYOUT_TIMING}`} />
    </>
  );
}

/** 분석 기간·조건 — 음악 리포트와 같은 줄(기간 고르기 + 직접 선택 달 칸 + 고르기 칸). 고른 조건은 주소에 남아
 *  분석 · 저작권료 내역 · CSV가 같은 조건을 쓴다 */
function RoyaltyFilterBar({ value, onChange, rows, works, album }: {
  value: RoyaltyFilter; onChange: (f: RoyaltyFilter) => void; rows: Royalty[]; works: Work[];
  /** 앨범 고르기도 같은 줄에 — 따로 한 줄을 차지하지 않게 */
  album: { value: string | null; onChange: (id: string | null) => void };
}) {
  const albums = useMemo(() => {
    const m = new Map<string, string>();
    for (const w of works) for (const r of w.recordings) if (!m.has(r.releaseId)) m.set(r.releaseId, r.releaseTitle);
    return [...m.entries()];
  }, [works]);
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
        {(albums.length > 1 || album.value) && (
          <select className="ui-select" aria-label="볼 앨범" value={album.value ?? ''} onChange={e => album.onChange(e.target.value || null)}>
            <option value="">모든 앨범</option>
            {albums.map(([id, title]) => <option key={id} value={id}>{title}</option>)}
          </select>
        )}
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

const setter = (params: URLSearchParams, setParams: (p: URLSearchParams, o?: { replace?: boolean }) => void) => (patch: Record<string, string | null>) => {
  const p = new URLSearchParams(params);
  for (const [k, v] of Object.entries(patch)) { if (v === null) p.delete(k); else p.set(k, v); }
  setParams(p, { replace: true });
};

/** '발매·곡 관리' › '곡 저작권' 탭 — 발매한 곡과 아직 발매하지 않은 곡의 작사·작곡 저작권을 한 목록에 */
/** 놓치고 있는 저작권료 — 배급 수익이 나는데 저작권 관리가 안 된 곡이 있는 앨범과 맡기면 받을 예상 금액 (홈 '확인할 작업'과 같은 계산) */
function MissedRoyalties({ works }: { works: Work[] }) {
  const { data: releases } = useAsync(() => api.listReleases(), [], { key: 'releases' });
  const { data: policy } = useAsync(() => pubApi.policy(), [], { key: 'pub-policy' });
  const { rows } = useReportRows();
  if (!releases || !policy) return null;
  const missed = missedRoyalties(releases, works, rows, policy.commissionRate);
  if (!missed.items.length) return null;
  return (
    <section className="ui-card aq-pbl-missed" aria-labelledby="pubMissed">
      <div className="ui-section-head"><h2 id="pubMissed">저작권료를 놓치고 있는 곡 {missed.songs}곡</h2></div>
      <p className="ui-field-help">배급 수익은 나는데 작사·작곡 저작권은 아직 맡기지 않았어요. 맡기면 분기마다 약 {money(missed.low)}~{money(missed.high)}을 더 받을 수 있어요 (최근 3개월 수익 기준 추정).</p>
      <div className="ui-list">
        {missed.items.map(m => (
          <Link key={m.releaseId} to={`/releases/${encodeURIComponent(m.releaseId)}?tab=publishing`} className="ui-row">
            <ReleaseCover id={m.releaseId} />
            <span className="min-0">
              <span className="ui-row-title">{m.title}</span>
              <span className="ui-row-meta"><span>{m.songs}곡 · 분기 약 {money(m.low)}~{money(m.high)}</span></span>
            </span>
            <Glyph name="chevron-right" size={16} />
          </Link>
        ))}
      </div>
    </section>
  );
}

export function PublishingSongs() {
  const nav = useNavigate();
  const [params, setParams] = useSearchParams();
  const set = setter(params, setParams);
  const { data: works } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  const releaseId = params.get('release');
  const scope: Scope | null = releaseId ? { releaseId, title: '' } : null;
  const songFilter = (params.get('wf') as WorkFilter | null) ?? 'all';
  return (
    <>
      <AlbumScope works={works ?? null} value={releaseId} onChange={id => set({ release: id })} />
      {!releaseId && works && <MissedRoyalties works={works} />}
      <Songs scope={scope} filter={songFilter} setFilter={f => set({ wf: f === 'all' ? null : f })}
        onApply={() => nav(`/publishing/apply${releaseId ? `?release=${encodeURIComponent(releaseId)}` : ''}`)} />
    </>
  );
}

/** '음악 리포트' › '저작권료' — 배급 수익 리포트와 같은 자리에서 기간·곡·징수 단체·국가로 좁혀 보고, 아래에 자세한 내역과 CSV */
export function PublishingReport() {
  const [params, setParams] = useSearchParams();
  const set = setter(params, setParams);
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
  // 분석 기간·조건 — 리포트 카드·곡별 금액·내역·CSV가 같은 조건을 쓴다
  const filter = useMemo(() => filterFromParams(params), [params]);
  const shownRows = useMemo(() => (scopedRows ? filterRoyalties(scopedRows, filter) : null), [scopedRows, filter]);
  // 아직 저작권료가 하나도 없으면 조건 이야기는 하지 않는다 ('최근 1년 기준 없음'이 아니라 '아직 없음')
  const filterLabel = scopedRows?.length ? royaltyFilterLabel(filter, { work: works?.find(w => w.id === filter.work)?.title }) : '';
  return (
    <div id="view-publishing" className="aq-pbl-report">
      {scopedRows && (scopedRows.length > 0 || releaseId) ? (
        <RoyaltyFilterBar value={filter} rows={scopedRows} works={works ?? []} onChange={f => set(filterToParams(f))}
          album={{ value: releaseId, onChange: id => set({ release: id, work: null }) }} />
      ) : <AlbumScope works={works ?? null} value={releaseId} onChange={id => set({ release: id, work: null })} />}
      <Overview scope={scope} works={works ?? null} rows={shownRows} label={filterLabel} />
      {!!scopedRows?.length && (
        <section className="aq-pbl-royalty-section" id="pubRoyaltyList" aria-labelledby="pubRoyaltyHead">
          <h2 id="pubRoyaltyHead" className="subhead">저작권료 내역</h2>
          <RoyaltiesTab rows={shownRows} label={filterLabel} />
        </section>
      )}
    </div>
  );
}

/** 예전 퍼블리싱 메뉴 주소 — 기능이 녹아든 자리로 (곡 → 발매·곡 관리, 분석·저작권료 → 음악 리포트, 계약서, Open A&R) */
export function Publishing() {
  const [params] = useSearchParams();
  const tab = params.get('tab');
  const p = new URLSearchParams(params);
  p.delete('tab');
  const to = tab === 'contracts' ? `/contracts${params.get('pub') ? `?pub=${encodeURIComponent(params.get('pub')!)}` : ''}`
    : tab === 'ar' ? `/ar${params.get('work') ? `?work=${encodeURIComponent(params.get('work')!)}` : ''}`
    : tab === 'songs' ? `/releases?tab=works${p.size ? `&${p}` : ''}`
    : `/reports?type=publishing${p.size ? `&${p}` : ''}`;
  return <Navigate to={to} replace />;
}
