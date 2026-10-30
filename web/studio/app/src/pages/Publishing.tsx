// 퍼블리싱 — 곡의 작사·작곡 저작권을 한 메뉴에서. 메뉴는 '퍼블리싱' 한 줄, 안에서 탭으로 나눈다.
//  개요(할 일·현황·진행 순서) · 곡(저작권 상태별 곡 목록) · 계약(신청·계약·연장) · 저작권료 · Open A&R
// 화면 모양은 발매·곡 관리와 같다: 머리(제목 + 주 버튼) → 탭 → 검색·칩 → 줄 목록. 배급 화면에는 이리로 오는 한 줄만 남긴다
import { useDeferredValue, useMemo, useState } from 'react';
import { Link, useNavigate, useSearchParams } from '../lib/router';
import { pubApi, type PublishingTask, type Work } from '../api/publishing';
import { useAsync } from '../hooks/useAsync';
import { APP_LABEL, APP_TONE, matchFilter, type WorkFilter } from '../lib/publishing';
import { Glyph, type GlyphName } from '../components/Glyph';
import { ReleaseCover } from '../components/ReleaseCover';
import { SkeletonRows } from '../components/Skeleton';
import { Money } from '../components/Money';
import { RegBadge } from '../components/publishing/parts';
import { PublishingContracts } from '../components/publishing/PublishingContracts';
import { RoyaltyTable } from '../components/publishing/Royalties';
import { OpenAr } from './OpenAr';
import { Badge, EmptyState, IconTile, StatGrid, StatTile, Tabs } from '../ui';

type Tab = 'overview' | 'songs' | 'contracts' | 'royalties' | 'ar';
const TABS: { value: Tab; label: string }[] = [
  { value: 'overview', label: '개요' }, { value: 'songs', label: '곡' }, { value: 'contracts', label: '계약' },
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

function Overview({ go }: { go: (t: Tab) => void }) {
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
        <StatGrid cols={2}>
          <StatTile label="관리 중인 곡" value={s.works.managed} unit="곡" hint={`전체 ${s.works.total}곡`} onClick={() => go('songs')} />
          <StatTile label="진행 중" value={s.works.inProgress} unit="곡" hint="심사·등록 준비" onClick={() => go('songs')} />
          <StatTile label="확정된 저작권료" value={<Money value={pubMoney.confirmed} />} hint={pubMoney.estimated ? <>예상 <Money value={pubMoney.estimated} /> 별도</> : '관리 단체 정산 기준'} onClick={() => go('royalties')} />
          <StatTile label="맡기지 않은 곡" value={s.works.unregistered} unit="곡" hint="신청하면 저작권료를 받아요" to="/publishing/apply" />
        </StatGrid>
      )}

      {(s.tasks.length > 0 || s.suggestion) && (
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

function RoyaltiesTab() {
  const { data: rows, loading } = useAsync(() => pubApi.listRoyalties(), [], { key: 'royalties:all' });
  if (loading && !rows) return <SkeletonRows count={3} pattern="amount" />;
  // 합계 칸은 RoyaltyTable이 그린다 (확정·예상·지급 완료) — 정산 잔액과 합쳐진다는 안내만 더한다
  return (
    <>
      <p className="ui-field-help">확정된 저작권료는 <Link to="/settlement?type=publishing" className="aq-inline-link">정산·지급</Link> 잔액에 배급 정산금과 함께 쌓여요.</p>
      <RoyaltyTable rows={rows ?? []} emptyText="관리 단체가 사용료를 나눠 주면 여기서 곡별로 보여 드려요. 보통 사용된 달부터 3~6개월 걸려요." />
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
  const { data: contracts } = useAsync(() => pubApi.listContracts(), [], { key: 'pub-contracts' });
  const toSign = (contracts ?? []).filter(c => c.status === 'pending_sign').length;

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
        items={TABS.map(t => ({ ...t, count: t.value === 'songs' ? works?.length ?? null : t.value === 'contracts' ? toSign || null : undefined }))} />

      <div className="ui-tab-panel" key={tab}>
        {tab === 'overview' && <Overview go={go} />}
        {tab === 'songs' && <Songs filter={filter} setFilter={f => set({ wf: f === 'all' ? null : f })} />}
        {tab === 'contracts' && (
          <div className="aq-bundle-list aq-stagger">
            <PublishingContracts all />
            {contracts && !contracts.length && (
              <EmptyState title="아직 저작권 관리 계약이 없어요" actions={<Link to="/publishing/apply" className="ui-btn is-primary">저작권 관리 신청</Link>}>
                신청이 승인되면 계약서가 여기에 정리돼요. 서명·연장·종료 요청도 여기서 해요.
              </EmptyState>
            )}
          </div>
        )}
        {tab === 'royalties' && <RoyaltiesTab />}
        {tab === 'ar' && <OpenAr embedded />}
      </div>
    </div>
  );
}
