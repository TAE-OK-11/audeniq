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
import { periodLabel } from '../data/reports';
import { money, num } from '../lib/format';
import { Badge, EmptyState, IconTile, KeyValue, Meter, Tabs } from '../ui';
import '../styles/split/Dashboard.css';

type Tab = 'overview' | 'songs' | 'royalties';
const TABS: { value: Tab; label: string }[] = [{ value: 'overview', label: '분석' }, { value: 'songs', label: '곡' }, { value: 'royalties', label: '저작권료' }];
/** 앨범 하나만 볼 때 — 그 앨범에 실린 곡과 그 곡들의 저작권료만 */
interface Scope { releaseId: string; title: string }
const inScope = (w: Work, scope: Scope | null) => !scope || w.recordings.some(r => r.releaseId === scope.releaseId);
const SONG_FILTERS: [WorkFilter, string][] = [['all', '전체'], ['managed', '관리 중'], ['review', '심사 중'], ['fix', '보완 필요'], ['none', '미신청'], ['unreleased', '미발매']];
const TASK_ICON: Record<PublishingTask['kind'], GlyphName> = { fix: 'warning', split: 'users', sign: 'sign', renew: 'repeat', ar: 'megaphone' };
const TASK_BTN: Record<PublishingTask['kind'], string> = { fix: '보완', split: '확인', sign: '서명', renew: '갱신', ar: '보기' };
const shortWon = (n: number) => (n >= 10000 ? `${(n / 10000).toFixed(n >= 100000 ? 0 : 1)}만` : num(n));
const nextMonth = (p: string) => { const [y, m] = p.split('-').map(Number); return m === 12 ? `${y + 1}-01` : `${y}-${String(m + 1).padStart(2, '0')}`; };
const topBy = (rows: readonly Royalty[], key: (r: Royalty) => string) => {
  const m = new Map<string, number>();
  for (const r of rows) m.set(key(r), (m.get(key(r)) ?? 0) + r.net);
  return [...m.entries()].sort((a, b) => b[1] - a[1])[0]?.[0];
};
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

function SongRow({ w }: { w: Work }) {
  const rec = w.recordings[0];
  return (
    <Link to={`/works/${w.id}`} className="track-row ui-row aq-track-row" aria-label={`${w.title || '제목 없는 곡'} 저작권 보기`}>
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
function Overview({ scope, works, rows }: { scope: Scope | null; works: Work[] | null; rows: Royalty[] | null }) {
  const nav = useNavigate();
  const { data: sum, loading } = useAsync(() => pubApi.summary(), [], { key: 'pub-summary' });
  if (loading && !sum) return <SkeletonRows count={3} />;
  if (!sum || !works) return null;
  // 앨범을 고르면 곡 수도 그 앨범 곡으로 센다 (할 일은 전체 기준이라 그때는 숨긴다)
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
        <EmptyState icon={<Glyph name="scale" size={22} />} title={scope ? '이 앨범에는 아직 정리한 곡이 없어요' : '아직 맡긴 곡이 없어요'}
          actions={<Link to={scope ? `/releases/${encodeURIComponent(scope.releaseId)}?tab=publishing` : '/publishing/apply'} className="ui-btn is-primary">{scope ? '앨범에서 시작하기' : '저작권 관리 신청'}</Link>}>
          곡을 쓴 사람에게는 작사·작곡 저작권이 있어요. 맡기면 방송·스트리밍 저작권료를 AUDENIQ이 받아 배급 정산과 같은 잔액으로 드려요.
        </EmptyState>
      ) : (
        <>
          {!scope && <Tasks s={s} nav={nav} />}
          <RoyaltyReport rows={rows} scope={scope} />
          {/* 정산·지급의 '받을 수 있는 금액'과 같은 카드 — 큰 숫자 하나 + 한 막대로 나눈 몫 + 범례 */}
          <section className="ui-card settle-hero" aria-labelledby="pubHeroSongs">
            <h2 id="pubHeroSongs">저작권 관리 중인 곡</h2>
            <strong className="settle-hero-amount">{s.works.managed}<span className="aq-money-unit">곡</span></strong>
            <div className="settle-split">
              <p className="settle-split-cap" aria-hidden="true">전체 <b>{s.works.total}곡</b> 중</p>
              <div className="settle-split-bar" aria-hidden="true">
                {/* 0곡인 몫은 그리지 않는다 — 빈 몫이 얇은 줄로 남지 않게 */}
                {s.works.managed > 0 && <i className="is-left" style={{ flexGrow: s.works.managed }} />}
                {s.works.inProgress > 0 && <i className="is-used" style={{ flexGrow: s.works.inProgress }} />}
                {s.works.unregistered > 0 && <i className="is-held" style={{ flexGrow: s.works.unregistered }} />}
              </div>
              <ul className="settle-split-legend">
                <li><i className="is-left" />관리 중 <b>{s.works.managed}곡</b></li>
                <li><i className="is-used" />심사·등록 준비 <b>{s.works.inProgress}곡</b></li>
                <li><i className="is-held" />맡기지 않음 <b>{s.works.unregistered}곡</b></li>
              </ul>
            </div>
            {/* 숫자만 보여 주고 끝내지 않는다 — 다음에 할 일 하나 */}
            {scope && (s.works.unregistered > 0 || s.works.inProgress > 0) ? (
              <Link to={`/releases/${encodeURIComponent(scope.releaseId)}?tab=publishing`} className="ui-btn is-soft is-block aq-pub-next">이 앨범 퍼블리싱 관리</Link>
            ) : s.works.unregistered > 0 ? (
              <Link to="/publishing?tab=songs&wf=none" className="ui-btn is-soft is-block aq-pub-next">맡기지 않은 곡 보기</Link>
            ) : s.works.inProgress > 0 ? (
              <Link to="/publishing?tab=songs&wf=review" className="ui-btn is-soft is-block aq-pub-next">심사 중인 곡 보기</Link>
            ) : null}
          </section>
          <SongRoyalties rows={rows} works={works} />
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
        <div className="aq-catalog-cards aq-stagger">{shown.map(w => <SongRow key={w.id} w={w} />)}</div>
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
function RoyaltyReport({ rows, scope }: { rows: Royalty[] | null; scope: Scope | null }) {
  const nav = useNavigate();
  const r = useMemo(() => {
    const list = rows ?? [];
    if (!list.length) return null;
    const by = new Map<string, number>();
    for (const x of list) by.set(x.period, (by.get(x.period) ?? 0) + x.net);
    const periods = [...by.keys()].sort();
    // 비는 달도 막대 자리는 남긴다 — 들어오지 않은 달이 그대로 보이게. 최근 여섯 달까지만
    const trend: { period: string; value: number }[] = [];
    for (let p = periods[0]; p <= periods[periods.length - 1]; p = nextMonth(p)) trend.push({ period: p, value: by.get(p) ?? 0 });
    const last = trend[trend.length - 1];
    const prev = trend[trend.length - 2];
    const lastRows = list.filter(x => x.period === last.period);
    return {
      period: last.period, amount: last.value, prev: prev?.value, trend: trend.slice(-6),
      estimated: lastRows.some(x => x.status === 'estimated'),
      topSong: topBy(list, x => x.workTitle), topSource: topBy(list, x => x.source),
    };
  }, [rows]);
  return (
    <section className="ui-card aq-report-card" aria-labelledby="pubReport">
      <div className="ui-section-head aq-report-head">
        <h2 id="pubReport">저작권료 리포트{r && <small>{periodLabel(r.period)}</small>}</h2>
        <button className="aq-head-link" type="button" onClick={() => nav(`/publishing?tab=royalties${scope ? `&release=${encodeURIComponent(scope.releaseId)}` : ''}`)}>내역 보기<Glyph name="chevron-right" size={14} /></button>
      </div>
      {!rows ? <SkeletonRows count={2} pattern="amount" /> : !r ? (
        <EmptyState compact icon={<Glyph name="chart" size={22} />} title="아직 들어온 저작권료가 없어요">
          관리 단체가 사용료를 나눠 주면 여기서 달마다 얼마나 벌었는지 보여 드려요. 보통 사용된 달부터 3~6개월 걸려요.
        </EmptyState>
      ) : (<>
        <div className="aq-report-total">
          <small>{Number(r.period.slice(5))}월분 저작권료</small>
          <strong><Money value={r.amount} animate /></strong>
          {(r.estimated || r.prev != null) && (
            <div className="aq-home-report-note">
              {r.estimated && <Badge tone="gray">확정 전 예상 포함</Badge>}
              {r.prev != null && <small>지난달 {money(r.prev)}</small>}
            </div>
          )}
        </div>
        <MiniTrend data={r.trend} label="최근 월별 저작권료 추이" short={shortWon} format={money} />
        <KeyValue className="aq-report-kv" cols={2} items={[
          ['많이 번 곡', <span className="aq-report-top"><IconTile size="sm"><Glyph name="music" size={16} /></IconTile><span>{r.topSong || '—'}</span></span>],
          ['많이 걷은 단체', <span className="aq-report-top"><IconTile size="sm"><Glyph name="globe" size={16} /></IconTile><span>{r.topSource || '—'}</span></span>],
        ]} />
        <p className="ui-row-meta aq-report-foot">관리 단체가 확정한 저작권료는 <Link to="/settlement?type=publishing" className="aq-inline-link">정산·지급</Link> 잔액에 배급 정산금과 함께 쌓여요.</p>
      </>)}
    </section>
  );
}

/** 곡별 저작권료 — 홈 '내 발매'와 같은 줄에 몫 막대만 더했다. 어느 곡이 가장 많이 벌었는지 (예상 제외) */
function SongRoyalties({ rows, works }: { rows: Royalty[] | null; works: Work[] | null }) {
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
      <div className="ui-section-head"><h2 id="pubBySong">곡별 저작권료</h2></div>
      <div className="ui-list aq-stagger">
        {top.map(s => (
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
    </section>
  );
}

/** 저작권료 — 기간별 자세한 내역과 CSV (합계·추이는 '분석' 탭) */
function RoyaltiesTab({ rows }: { rows: Royalty[] | null }) {
  if (!rows) return <SkeletonRows count={3} pattern="amount" />;
  return (
    <>
      <p className="ui-field-help">확정된 저작권료는 <Link to="/settlement?type=publishing" className="aq-inline-link">정산·지급</Link> 잔액에 배급 정산금과 함께 쌓여요.</p>
      <RoyaltyTable summary={false} rows={rows} emptyText="관리 단체가 사용료를 나눠 주면 여기서 곡별로 보여 드려요. 보통 사용된 달부터 3~6개월 걸려요." />
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
  const filter = (params.get('wf') as WorkFilter | null) ?? 'all';
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
        <button type="button" className="ui-btn is-primary" onClick={() => nav('/publishing/apply')}><Glyph name="plus" size={14} />저작권 관리 신청</button>
      </div>

      <Tabs label="퍼블리싱 메뉴" value={tab} onChange={go}
        items={TABS.map(t => ({ ...t, count: t.value === 'songs' ? works?.length ?? null : undefined }))} />

      <AlbumScope works={works ?? null} value={releaseId} onChange={id => set({ release: id })} />

      <div className="ui-tab-panel" key={tab}>
        {tab === 'overview' && <Overview scope={scope} works={works ?? null} rows={scopedRows} />}
        {tab === 'songs' && <Songs scope={scope} filter={filter} setFilter={f => set({ wf: f === 'all' ? null : f })} />}
        {tab === 'royalties' && <RoyaltiesTab rows={scopedRows} />}
      </div>
    </div>
  );
}
