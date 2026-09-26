import { useDeferredValue, useMemo, useState } from 'react';
import { useNavigate, useSearchParams } from 'react-router';
import { api, type Track } from '../api/client';
import { STATUS_LABEL } from '../lib/format';
import { useAsync } from '../hooks/useAsync';
import { SkeletonRows } from '../components/Skeleton';
import { ReleaseCover } from '../components/ReleaseCover';

const FILTERS = [
  { value: 'all', label: '전체' },
  { value: 'draft', label: '작성 중' },
  { value: 'ready', label: '접수 대기' },
  { value: 'review', label: '검토 중' },
  { value: 'needs', label: '보완 필요' },
  { value: 'scheduled', label: '발매 예정' },
  { value: 'live', label: '발매 완료' },
];

const SORTS = [
  { value: 'updated', label: '최근 수정순' },
  { value: 'date', label: '발매일순' },
  { value: 'title', label: '이름순' },
];

type TrackWithRelease = Track & { releaseTitle: string; releaseId: string; artist?: string; coverData?: string };

const norm = (s: string | null | undefined) => (s ?? '').toLowerCase().replace(/\s+/g, '');

export function Releases() {
  const nav = useNavigate();
  const [params, setParams] = useSearchParams();
  const tab = params.get('tab') === 'tracks' ? 'tracks' : 'releases';
  const filter = params.get('status') || 'all';
  const [query, setQuery] = useState('');
  const [sort, setSort] = useState('updated');
  const [trackQuery, setTrackQuery] = useState('');
  const dq = useDeferredValue(query);
  const dtq = useDeferredValue(trackQuery);

  // 탭·필터는 주소에 남겨 새로고침·뒤로 가기에도 유지
  const setParam = (key: string, value: string, fallback: string) => {
    const next = new URLSearchParams(params);
    if (value === fallback) next.delete(key); else next.set(key, value);
    setParams(next, { replace: true });
  };

  const { data, loading, error, reload } = useAsync(async () => {
    const list = await api.listReleases();
    const details = await Promise.all(list.map(r => api.getRelease(r.id).catch(() => null)));
    const tracks: TrackWithRelease[] = [];
    for (const d of details) {
      if (!d) continue;
      for (const t of d.tracks) {
        tracks.push({ ...t, releaseTitle: d.title, releaseId: d.id, artist: d.artist, coverData: d.draft?.coverData });
      }
    }
    return { list, tracks };
  }, []);
  const list = useMemo(() => data?.list ?? [], [data]);
  const tracks = useMemo(() => data?.tracks ?? [], [data]);

  const counts = useMemo(() => {
    const c: Record<string, number> = { all: list.length };
    for (const r of list) c[r.status || 'draft'] = (c[r.status || 'draft'] || 0) + 1;
    return c;
  }, [list]);

  const filtered = useMemo(() => {
    const q = norm(dq);
    const out = list.filter(r =>
      (filter === 'all' || (r.status || 'draft') === filter)
      && (!q || norm(r.title).includes(q) || norm(r.artist).includes(q)));
    if (sort === 'title') out.sort((a, b) => a.title.localeCompare(b.title, 'ko'));
    else if (sort === 'date') out.sort((a, b) => String(b.release_date || '').localeCompare(String(a.release_date || '')));
    return out;
  }, [list, filter, dq, sort]);

  const filteredTracks = useMemo(() => {
    const q = norm(dtq);
    if (!q) return tracks;
    return tracks.filter(t =>
      norm(t.title).includes(q) || norm(t.isrc).includes(q) || norm(t.artist).includes(q) || norm(t.releaseTitle).includes(q));
  }, [tracks, dtq]);

  const openRelease = (id: string) => nav(`/releases/${id}`);
  const onCardKey = (id: string) => (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); openRelease(id); }
  };

  return (
    <div id="view-catalog" className="view">
      <div className="view-title">
        <div>
          <p className="eyebrow">CATALOG</p>
          <h1 id="catalogTitle">내 음악</h1>
          <p>발매한 음악과 심사 진행 상황을 한눈에 확인해 보세요.</p>
        </div>
        <button type="button" className="button" onClick={() => nav('/upload')}>새 발매</button>
      </div>

      <div className="tabs aq-tabs" role="tablist" aria-label="카탈로그 메뉴">
        <button
          type="button" role="tab" className="tab" aria-selected={tab === 'releases'}
          onClick={() => setParam('tab', 'releases', 'releases')}
        >발매 목록 {!loading && <small className="aq-tab-count">{list.length}</small>}</button>
        <button
          type="button" role="tab" className="tab" aria-selected={tab === 'tracks'}
          onClick={() => setParam('tab', 'tracks', 'releases')}
        >전체 트랙 {!loading && <small className="aq-tab-count">{tracks.length}</small>}</button>
      </div>

      {loading ? (
        <SkeletonRows count={4} />
      ) : error ? (
        <div className="empty-page">
          <h2>발매 목록을 불러오지 못했어요.</h2>
          <p>{error}</p>
          <button type="button" className="button secondary" onClick={reload}>다시 불러오기</button>
        </div>
      ) : tab === 'releases' ? (
        <div className="aq-tab-panel" key="releases">
          <div className="toolbar">
            <input
              id="catalogSearch" type="search" aria-label="발매 검색" placeholder="발매명·아티스트 검색"
              value={query} onChange={e => setQuery(e.target.value)}
            />
            <select id="catalogSort" aria-label="정렬" value={sort} onChange={e => setSort(e.target.value)}>
              {SORTS.map(o => <option key={o.value} value={o.value}>{o.label}</option>)}
            </select>
          </div>
          <div className="studio-filterrail" id="catalogFilterRail" role="group" aria-label="발매 진행 상태">
            {FILTERS.map(f => {
              const active = filter === f.value;
              const count = counts[f.value] || 0;
              if (f.value !== 'all' && !count && !active) return null;
              return (
                <button
                  key={f.value} type="button"
                  className={`studio-filter${active ? ' active' : ''}`}
                  aria-pressed={active}
                  onClick={() => setParam('status', f.value, 'all')}
                >
                  {f.label}
                  <small>{count}</small>
                </button>
              );
            })}
          </div>
          <div className="studio-result-count" id="catalogCount" aria-live="polite">{filtered.length}개의 발매</div>
          {filtered.length ? (
            <div className="aq-catalog-cards aq-stagger">
              {filtered.map(r => (
                <article
                  key={r.id} className="aq-release-card"
                  onClick={() => openRelease(r.id)} onKeyDown={onCardKey(r.id)}
                  tabIndex={0} role="link" aria-label={`${r.title || '제목 없는 발매'} 상세 보기`}
                >
                  <ReleaseCover id={r.id} src={r.coverData} />
                  <div className="min-0">
                    <span className="row-name">{r.title || '제목 없는 발매'}</span>
                    <span className="row-sub">{r.artist || '아티스트 미입력'} · {r.track_count}곡</span>
                    <span className="row-sub">{r.release_date ?? '발매일 미정'}</span>
                  </div>
                  <div className="aq-release-end">
                    <span className={`status-chip ${r.status || 'draft'}`}>{STATUS_LABEL[r.status] || r.status}</span>
                  </div>
                </article>
              ))}
            </div>
          ) : (
            <div className="empty-page">
              <h2>{query || filter !== 'all' ? '조건에 맞는 발매가 없어요.' : '아직 등록한 발매가 없어요.'}</h2>
              <p>{query || filter !== 'all' ? '검색어나 상태 필터를 바꿔 보세요.' : '발매 정보를 저장하면 이곳에서 수정하고 관리할 수 있어요.'}</p>
              {(query || filter !== 'all') ? (
                <button type="button" className="button secondary" onClick={() => { setQuery(''); setParam('status', 'all', 'all'); }}>필터 초기화</button>
              ) : (
                <button type="button" className="button" onClick={() => nav('/upload')}>첫 발매 등록하기</button>
              )}
            </div>
          )}
        </div>
      ) : (
        <div className="aq-tab-panel" key="tracks">
          <div className="toolbar">
            <input
              id="trackSearch" type="search" aria-label="트랙 검색" placeholder="곡명·아티스트·ISRC 검색"
              value={trackQuery} onChange={e => setTrackQuery(e.target.value)}
            />
          </div>
          <div className="studio-result-count" aria-live="polite">{filteredTracks.length}개의 트랙</div>
          {filteredTracks.length ? (
            <div className="aq-catalog-cards aq-stagger">
              {filteredTracks.map(t => (
                <div
                  key={t.releaseId + t.id} className="track-row aq-track-row"
                  onClick={() => openRelease(t.releaseId)} onKeyDown={onCardKey(t.releaseId)}
                  tabIndex={0} role="link" aria-label={`${t.title || '제목 없는 곡'} 상세 보기`}
                >
                  <ReleaseCover id={t.releaseId} src={t.coverData} />
                  <div className="min-0">
                    <span className="row-name">{t.title || '제목 없는 곡'}{t.explicit && <em className="aq-explicit" title="Explicit">E</em>}</span>
                    <span className="row-sub">{t.releaseTitle} · {t.artist || '아티스트 미입력'}{t.isrc ? ` · ISRC ${t.isrc}` : ''}</span>
                  </div>
                  <span className="chevron" aria-hidden="true">›</span>
                </div>
              ))}
            </div>
          ) : (
            <div className="empty-page">
              <h2>{trackQuery ? '검색 결과가 없어요.' : '표시할 트랙이 없어요.'}</h2>
              <p>{trackQuery ? '다른 검색어로 찾아보세요.' : '새로운 발매를 등록하면 곡별 정보를 볼 수 있어요.'}</p>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
