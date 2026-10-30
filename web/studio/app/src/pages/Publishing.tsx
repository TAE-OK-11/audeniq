// 퍼블리싱 — 메뉴의 '퍼블리싱'은 전체를 한눈에 보는 곳: 분석(현황·앨범별·할 일) · 곡 · 저작권료 · Open A&R.
// 앨범 하나의 퍼블리싱 설정은 발매 상세 '관리' 탭의 선택 카드 → 앨범 퍼블리싱 화면(/releases/:id/publishing)에서 한다.
// 계약서·정산·알림·문의·계정은 배급과 같은 화면을 함께 쓴다 (여기에 따로 두지 않는다)
import { useDeferredValue, useMemo, useState } from 'react';
import { Link, Navigate, useNavigate, useSearchParams } from '../lib/router';
import { pubApi, type PublishingTask, type Work } from '../api/publishing';
import { useAsync } from '../hooks/useAsync';
import { APP_LABEL, APP_TONE, matchFilter, type WorkFilter } from '../lib/publishing';
import { Glyph, type GlyphName } from '../components/Glyph';
import { ReleaseCover } from '../components/ReleaseCover';
import { SkeletonRows } from '../components/Skeleton';
import { Money } from '../components/Money';
import { RegBadge } from '../components/publishing/parts';
import { albumPublishing } from '../components/publishing/ReleasePublishing';
import { api } from '../api/client';
import { RoyaltyTable } from '../components/publishing/Royalties';
import { OpenAr } from './OpenAr';
import { Badge, EmptyState, IconTile, Meter, Tabs } from '../ui';

// 탭마다 한 가지 일만 — 분석은 숫자만, 할 일·앨범은 '앨범', 곡 목록은 '곡', 내역은 '저작권료'
type Tab = 'overview' | 'albums' | 'songs' | 'royalties' | 'ar';
const TABS: { value: Tab; label: string }[] = [
  { value: 'overview', label: '분석' }, { value: 'albums', label: '앨범' }, { value: 'songs', label: '곡' },
  { value: 'royalties', label: '저작권료' }, { value: 'ar', label: 'Open A&R' },
];
const SONG_FILTERS: [WorkFilter, string][] = [['all', '전체'], ['managed', '관리 중'], ['review', '심사 중'], ['fix', '보완 필요'], ['none', '미신청'], ['unreleased', '미발매']];
const TASK_ICON: Record<PublishingTask['kind'], GlyphName> = { fix: 'alert', split: 'users', sign: 'sign', renew: 'repeat', ar: 'megaphone' };
const PATH = [
  ['저작권 관리 신청', '맡길 곡과 지분을 고르고 신청서에 서명'],
  ['담당자 심사', '참여자·지분·권리를 확인해요'],
  ['계약서 서명', '수수료·관리 범위를 확인하고 서명'],
  ['관리 단체 등록', 'KOMCA 등 관리 단체에 곡을 등록해요'],
  ['저작권료 정산', '들어온 저작권료는 배급 정산과 같은 잔액으로'],
] as const;
const norm = (s: string) => s.toLowerCase().replace(/\s+/g, '');

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

/** 분석 — 숫자만: 관리 중인 곡 · 확정된 저작권료 · 곡별 저작권료 */
function Overview() {
  const { data: s, loading } = useAsync(() => pubApi.summary(), [], { key: 'pub-summary' });
  if (loading && !s) return <SkeletonRows count={3} />;
  if (!s) return null;
  const pubMoney = s.revenue.publishing;
  return (
    <div className="aq-detail-stack">
      {!s.works.total ? (
        <EmptyState icon={<Glyph name="scale" size={22} />} title="아직 맡긴 곡이 없어요"
          actions={<Link to="/publishing/apply" className="ui-btn is-primary">저작권 관리 신청</Link>}>
          곡을 쓴 사람에게는 작사·작곡 저작권이 있어요. 맡기면 방송·스트리밍 저작권료를 AUDENIQ이 받아 배급 정산과 같은 잔액으로 드려요.
        </EmptyState>
      ) : (
        <>
          {/* 정산·지급의 '받을 수 있는 금액'과 같은 카드 — 큰 숫자 하나 + 한 막대로 나눈 몫 + 범례 */}
          <section className="ui-card settle-hero" aria-labelledby="pubHeroSongs">
            <h2 id="pubHeroSongs">저작권 관리 중인 곡</h2>
            <strong className="settle-hero-amount">{s.works.managed}<span className="aq-money-unit">곡</span></strong>
            <div className="settle-split">
              <p className="settle-split-cap" aria-hidden="true">전체 <b>{s.works.total}곡</b> 중</p>
              <div className="settle-split-bar" aria-hidden="true">
                <i className="is-left" style={{ flexGrow: s.works.managed }} />
                <i className="is-used" style={{ flexGrow: s.works.inProgress }} />
                <i className="is-held" style={{ flexGrow: s.works.unregistered }} />
              </div>
              <ul className="settle-split-legend">
                <li><i className="is-left" />관리 중 <b>{s.works.managed}곡</b></li>
                <li><i className="is-used" />심사·등록 준비 <b>{s.works.inProgress}곡</b></li>
                <li><i className="is-held" />맡기지 않음 <b>{s.works.unregistered}곡</b></li>
              </ul>
            </div>
          </section>
          <section className="ui-card settle-hero" aria-labelledby="pubHeroMoney">
            <h2 id="pubHeroMoney">확정된 저작권료</h2>
            <strong className="settle-hero-amount"><Money value={pubMoney.confirmed} animate /></strong>
            <p className="settle-hero-sub">{pubMoney.estimated ? <>확정 전 예상 <Money value={pubMoney.estimated} />은 관리 단체가 확정하면 적립돼요. </> : ''}배급 정산금과 같은 잔액에 쌓여요.</p>
          </section>
          <SongRoyaltyBars />
        </>
      )}
    </div>
  );
}

/** 앨범 — 할 일과 앨범별 현황 (누르면 그 앨범의 퍼블리싱 설정), 처음 쓰는 사람을 위한 진행 순서 */
function Albums() {
  const { data: s } = useAsync(() => pubApi.summary(), [], { key: 'pub-summary' });
  return (
    <div className="aq-detail-stack">

      {s && (s.tasks.length > 0 || s.suggestion) && (
        <section className="ui-card" aria-labelledby="pubTasks">
          <h2 id="pubTasks" className="subhead">확인할 일</h2>
          <div className="ui-list is-inset">
            {s.tasks.map(t => (
              <Link key={t.id} to={t.to} className="ui-row">
                <IconTile size="sm" tone={t.kind === 'fix' ? 'amber' : undefined}><Glyph name={TASK_ICON[t.kind]} size={16} /></IconTile>
                <span className="min-0"><span className="ui-row-title">{t.title}</span><span className="ui-row-meta"><span>{t.sub}</span></span></span>
                <Glyph name="chevron-right" size={16} />
              </Link>
            ))}
            {s.suggestion && (
              <Link to={`/works/new?release=${encodeURIComponent(s.suggestion.releaseId)}&track=${encodeURIComponent(s.suggestion.trackId)}`} className="ui-row">
                <IconTile size="sm"><Glyph name="plus" size={16} /></IconTile>
                <span className="min-0">
                  <span className="ui-row-title">‘{s.suggestion.title}’ 저작권도 맡길까요?</span>
                  <span className="ui-row-meta"><span>{s.suggestion.releaseTitle}에 실린 곡이에요</span></span>
                </span>
                <Glyph name="chevron-right" size={16} />
              </Link>
            )}
          </div>
        </section>
      )}

      <AlbumList />

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
    </div>
  );
}

/** 앨범별 — 앨범마다 몇 곡을 맡겼는지. 누르면 그 앨범의 퍼블리싱 설정 */
function AlbumList() {
  const { data: works } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  const { data: releases } = useAsync(() => api.listReleases(), [], { key: 'releases' });
  if (!works || !releases) return null;
  const rows = releases.filter(r => r.status !== 'draft' && (r.track_count ?? 0) > 0).map(r => {
    const a = albumPublishing(works, r.id, r.track_count ?? 0);
    return { r, a };
  });
  if (!rows.length) return null;
  return (
    <section className="ui-card" aria-labelledby="pubAlbums">
      <h2 id="pubAlbums" className="subhead">앨범별</h2>
      <div className="ui-list is-inset">
        {rows.map(({ r, a }) => (
          <Link key={r.id} to={`/releases/${r.id}/publishing`} className="ui-row">
            <ReleaseCover id={r.id} src={r.coverData} className="cover aq-cover is-sm" />
            <span className="min-0">
              <span className="ui-row-title">{r.title}</span>
              <span className="ui-row-meta"><span>{r.track_count}곡 중 {a.managed.length}곡 관리 중</span></span>
            </span>
            {a.status ? <Badge tone={a.status.tone}>{a.status.label}</Badge> : <Badge tone="gray">미신청</Badge>}
          </Link>
        ))}
      </div>
    </section>
  );
}

function Songs({ filter, setFilter }: { filter: WorkFilter; setFilter: (f: WorkFilter) => void }) {
  const nav = useNavigate();
  const { data: works, loading } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  const [query, setQuery] = useState('');
  const q = norm(useDeferredValue(query));
  const shown = useMemo(() => (works ?? []).filter(w => matchFilter(w, filter)
    && (!q || norm(w.title).includes(q) || w.contributors.some(c => norm(c.name).includes(q)) || w.recordings.some(r => norm(r.releaseTitle).includes(q)))), [works, filter, q]);
  return (
    <>
      <div className="toolbar">
        <input id="pubSongSearch" className="ui-input" type="search" aria-label="곡 검색" placeholder="곡·참여자·발매 검색" value={query} onChange={e => setQuery(e.target.value)} />
        {/* 발매 전 곡도 작사·작곡 정보와 지분을 먼저 정리해 둘 수 있다 */}
        <button type="button" className="ui-btn is-soft" onClick={() => nav('/works/new')}><Glyph name="plus" size={14} />곡 추가</button>
      </div>
      <div className="ui-chips" role="group" aria-label="저작권 상태">
        {SONG_FILTERS.map(([v, label]) => (
          <button key={v} type="button" className={`ui-chip${filter === v ? ' is-on' : ''}`} aria-pressed={filter === v} onClick={() => setFilter(v)}>{label}</button>
        ))}
      </div>
      <div className="studio-result-count" aria-live="polite">{works ? `${shown.length}개의 곡` : ''}</div>
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

/** 곡별 저작권료 막대 — 어느 곡이 저작권료를 가장 많이 벌었는지 (예상 제외) */
function SongRoyaltyBars() {
  const { data: rows } = useAsync(() => pubApi.listRoyalties(), [], { key: 'royalties:all' });
  const bySong = new Map<string, { title: string; id: string; net: number }>();
  for (const r of rows ?? []) {
    if (r.status === 'estimated') continue;
    const cur = bySong.get(r.workId) ?? { title: r.workTitle, id: r.workId, net: 0 };
    cur.net += r.net; bySong.set(r.workId, cur);
  }
  const top = [...bySong.values()].sort((a, b) => b.net - a.net);
  const max = top[0]?.net ?? 0;
  if (!top.length) return null;
  return (
    <section className="ui-card aq-pub-songs" aria-labelledby="pubBySong">
      <h2 id="pubBySong" className="subhead">곡별 저작권료</h2>
      <div className="ui-list is-inset">
        {top.map(s => (
          <Link key={s.id} to={`/works/${s.id}?tab=revenue`} className="ui-row">
            <span className="min-0">
              <span className="ui-row-title">{s.title}</span>
              <Meter parts={[{ value: s.net }]} total={max} label={`${s.title} 저작권료`} />
            </span>
            <b className="aq-pub-amount"><Money value={s.net} /></b>
          </Link>
        ))}
      </div>
    </section>
  );
}

/** 저작권료 — 기간별 내역만 (합계·곡별 분석은 '분석' 탭) */
function RoyaltiesTab() {
  const { data: rows, loading } = useAsync(() => pubApi.listRoyalties(), [], { key: 'royalties:all' });
  if (loading && !rows) return <SkeletonRows count={3} pattern="amount" />;
  return (
    <>
      <p className="ui-field-help">확정된 저작권료는 <Link to="/settlement?type=publishing" className="aq-inline-link">정산·지급</Link> 잔액에 배급 정산금과 함께 쌓여요.</p>
      <RoyaltyTable summary={false} rows={rows ?? []} emptyText="관리 단체가 사용료를 나눠 주면 여기서 곡별로 보여 드려요. 보통 사용된 달부터 3~6개월 걸려요." />
    </>
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
  const go = (t: Tab) => set({ tab: t === 'overview' ? null : t, wf: null });
  const { data: works } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  // 계약은 배급과 같은 계약서 화면에서 — 예전 '계약' 탭 주소는 그리로
  if (params.get('tab') === 'contracts') return <Navigate to={`/contracts${params.get('pub') ? `?pub=${encodeURIComponent(params.get('pub')!)}` : ''}`} replace />;

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

      <div className="ui-tab-panel" key={tab}>
        {tab === 'overview' && <Overview />}
        {tab === 'albums' && <Albums />}
        {tab === 'songs' && <Songs filter={filter} setFilter={f => set({ wf: f === 'all' ? null : f })} />}
        {tab === 'royalties' && <RoyaltiesTab />}
        {tab === 'ar' && <OpenAr embedded />}
      </div>
    </div>
  );
}
