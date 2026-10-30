// 음악 라이브러리의 '저작물' 보기 — 발매·트랙 목록 옆 탭 (기획안 2.2).
// 아직 발매하지 않은 곡도 작품으로 둘 수 있고, 퍼블리싱 상태별로 거른다. 필터는 주소에 남는다
import { useDeferredValue, useMemo, useState } from 'react';
import { Link, useSearchParams } from '../../lib/router';
import { pubApi } from '../../api/publishing';
import { useAsync } from '../../hooks/useAsync';
import { WORK_FILTERS, matchFilter, type WorkFilter } from '../../lib/publishing';
import { SkeletonRows } from '../Skeleton';
import { Glyph } from '../Glyph';
import { WorkRow } from './parts';
import { Button, ChipGroup, EmptyState, StatGrid, StatTile } from '../../ui';

const norm = (s: string | null | undefined) => (s ?? '').toLowerCase().replace(/\s+/g, '');

export function WorkLibrary() {
  const [params, setParams] = useSearchParams();
  const filter = (WORK_FILTERS.some(([v]) => v === params.get('wf')) ? params.get('wf') : 'all') as WorkFilter;
  const setFilter = (v: WorkFilter) => { const p = new URLSearchParams(params); if (v === 'all') p.delete('wf'); else p.set('wf', v); setParams(p, { replace: true }); };
  const [query, setQuery] = useState('');
  const dq = useDeferredValue(query);
  const { data, loading, error, reload } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  const works = useMemo(() => data ?? [], [data]);

  const counts = useMemo(() => Object.fromEntries(WORK_FILTERS.map(([v]) => [v, works.filter(w => matchFilter(w, v)).length])) as Record<WorkFilter, number>, [works]);
  const shown = useMemo(() => {
    const q = norm(dq);
    return works.filter(w => matchFilter(w, filter) && (!q
      || norm(w.title).includes(q) || w.altTitles.some(t => norm(t).includes(q)) || norm(w.iswc).includes(q) || norm(w.internalCode).includes(q)
      || w.contributors.some(c => norm(c.name).includes(q))));
  }, [works, filter, dq]);

  if (loading && !data) return <SkeletonRows count={4} />;
  if (error) {
    return <div className="ui-empty"><h2>작품 목록을 불러오지 못했어요.</h2><p>{error}</p><Button onClick={reload}>다시 불러오기</Button></div>;
  }

  return (
    <div className="ui-tab-panel" key="works">
      {works.length > 0 && (
        <StatGrid cols={3}>
          <StatTile compact label="전체 작품" value={works.length} unit="개" tone={filter === 'all' ? 'active' : undefined} onClick={() => setFilter('all')} />
          <StatTile compact label="퍼블리싱 관리 중" value={counts.managed} unit="개" tone={filter === 'managed' ? 'active' : undefined} onClick={() => setFilter('managed')} />
          <StatTile compact label="보완 필요" value={counts.fix} unit="개" tone={counts.fix ? 'alert' : 'muted'} onClick={() => setFilter('fix')} />
        </StatGrid>
      )}
      <div className="toolbar">
        <input className="ui-input" type="search" aria-label="작품 검색" placeholder="작품명·참여자·ISWC 검색" value={query} onChange={e => setQuery(e.target.value)} />
        <Link to="/works/new" className="ui-btn is-soft"><Glyph name="plus" size={14} />새 작품</Link>
      </div>
      <ChipGroup label="작품 거르기" value={filter} onChange={setFilter} options={WORK_FILTERS.map(([v, l]) => ({ value: v, label: l, count: counts[v], hidden: v !== 'all' && v !== filter && !counts[v] }))} />
      <div className="studio-result-count" aria-live="polite">{shown.length}개의 작품</div>
      {shown.length ? (
        <div className="ui-list aq-stagger">
          {shown.map(w => <WorkRow key={w.id} work={w} to={`/works/${w.id}`} />)}
        </div>
      ) : works.length ? (
        <EmptyState title="조건에 맞는 작품이 없어요" actions={<Button onClick={() => { setQuery(''); setFilter('all'); }}>필터 초기화</Button>}>검색어나 필터를 바꿔 보세요.</EmptyState>
      ) : (
        <EmptyState icon={<Glyph name="pencil" size={22} />} title="아직 등록한 작품이 없어요"
          actions={<><Link to="/works/new" className="ui-btn is-primary">새 작품 등록</Link><Link to="/releases?tab=tracks" className="ui-btn is-soft">발매 곡에서 만들기</Link></>}>
          작사·작곡한 곡을 작품으로 등록하면 지분과 권리를 한곳에서 관리해요. 아직 발매하지 않은 곡도 돼요.
        </EmptyState>
      )}
    </div>
  );
}
