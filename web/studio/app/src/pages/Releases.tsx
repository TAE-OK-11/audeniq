import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { mockApi } from '../api/mock';
import type { Release, Track } from '../api/client';
import { STATUS_LABEL } from '../lib/format';

const FILTERS = [
  { value: 'all', label: '전체' },
  { value: 'draft', label: '작성 중' },
  { value: 'ready', label: '접수 대기' },
  { value: 'review', label: '검토 중' },
  { value: 'needs', label: '보완 필요' },
  { value: 'scheduled', label: '발매 예정' },
  { value: 'live', label: '발매 완료' },
];

const SELECT_OPTIONS = [
  { value: 'all', label: '전체 상태' },
  { value: 'scheduled', label: '발매 예정' },
  { value: 'draft', label: '임시 저장' },
  { value: 'ready', label: '접수 대기' },
  { value: 'needs', label: '보완 필요' },
  { value: 'review', label: '검토 중' },
  { value: 'live', label: '발매 완료' },
];

type TrackWithRelease = Track & { releaseTitle: string; releaseId: string; artist?: string };

const COVER_GRADIENTS = [
  'linear-gradient(135deg,#bcd8ff,#7162db 65%,#394b83)',
  'linear-gradient(135deg,#ffd9c1,#e58a7a 65%,#8a3d4b)',
  'linear-gradient(135deg,#bdf0d9,#4dae8a 65%,#2a6b52)',
  'linear-gradient(135deg,#ecd9ff,#a97ae5 65%,#5b3d8a)',
  'linear-gradient(135deg,#fff0b8,#e0a83f 65%,#8a6a2a)',
  'linear-gradient(135deg,#b8e6ff,#5c9ae5 65%,#2a4d8a)',
  'linear-gradient(135deg,#ffcfe3,#e57aa5 65%,#8a3d63)',
];

function gradientFor(id: string): string {
  let h = 0;
  for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) >>> 0;
  return COVER_GRADIENTS[h % COVER_GRADIENTS.length];
}

function Cover({ id }: { id?: string }) {
  return (
    <span className="cover aq-cover" aria-hidden="true" style={id ? { background: gradientFor(id) } : undefined}>♫</span>
  );
}

export function Releases() {
  const nav = useNavigate();
  const [tab, setTab] = useState<'releases' | 'tracks'>('releases');
  const [query, setQuery] = useState('');
  const [filter, setFilter] = useState('all');
  const [trackQuery, setTrackQuery] = useState('');
  const [list, setList] = useState<Release[]>([]);
  const [tracks, setTracks] = useState<TrackWithRelease[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState(false);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const rs = await mockApi.listReleases();
        if (cancelled) return;
        setList(rs);
        // N+1 순차 호출 → 병렬화 (latency가 발매 수에 비례해 누적되는 문제 해결)
        const details = await Promise.all(
          rs.map(r => mockApi.getRelease(r.id).catch(() => null)),
        );
        if (cancelled) return;
        const all: TrackWithRelease[] = [];
        for (const d of details) {
          if (!d) continue;
          for (const t of d.tracks) {
            all.push({ ...t, releaseTitle: d.title, releaseId: d.id, artist: d.artist });
          }
        }
        setTracks(all);
      } catch {
        if (!cancelled) setLoadError(true);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => { cancelled = true; };
  }, []);

  const chipOf = (status: string) => status || 'draft';
  const labelOf = (status: string) => STATUS_LABEL[status] || status;

  const filtered = list.filter(r => {
    const matchFilter = filter === 'all' || chipOf(r.status) === filter;
    const matchQuery = !query || r.title.toLowerCase().includes(query.toLowerCase());
    return matchFilter && matchQuery;
  });

  const filteredTracks = tracks.filter(t => {
    if (!trackQuery) return true;
    const q = trackQuery.toLowerCase();
    return t.title.toLowerCase().includes(q) || (t.isrc ?? '').toLowerCase().includes(q);
  });

  const openRelease = (id: string) => nav(`/releases/${id}`);

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

      <div className="tabs" role="tablist" aria-label="카탈로그 메뉴">
        <button
          type="button" className="tab" data-cat-tab="releases"
          aria-selected={tab === 'releases'}
          onClick={() => setTab('releases')}
        >발매 목록</button>
        <button
          type="button" className="tab" data-cat-tab="tracks"
          aria-selected={tab === 'tracks'}
          onClick={() => setTab('tracks')}
        >전체 트랙</button>
      </div>

      {loading ? (
        <p style={{ color: 'var(--muted)' }}>불러오는 중...</p>
      ) : loadError ? (
        <div className="empty-page">
          <h2>발매 목록을 불러오지 못했어요.</h2>
          <p>네트워크 연결을 확인하고 다시 시도해 주세요.</p>
          <button type="button" className="button secondary" onClick={() => window.location.reload()}>다시 불러오기</button>
        </div>
      ) : tab === 'releases' ? (
        <div>
          <div className="toolbar">
            <input
              id="catalogSearch" type="search" aria-label="발매 검색" placeholder="발매명·아티스트 검색"
              value={query} onChange={e => setQuery(e.target.value)}
            />
            <select id="catalogFilter" aria-label="발매 상태" value={filter} onChange={e => setFilter(e.target.value)}>
              {SELECT_OPTIONS.map(o => <option key={o.value} value={o.value}>{o.label}</option>)}
            </select>
          </div>
          <div className="studio-filterrail" id="catalogFilterRail" role="group" aria-label="발매 진행 상태">
            {FILTERS.map(f => {
              const active = filter === f.value;
              const count = f.value === 'all'
                ? list.length
                : list.filter(r => chipOf(r.status) === f.value).length;
              return (
                <button
                  key={f.value} type="button"
                  className={`studio-filter${active ? ' active' : ''}`}
                  data-status-filter={f.value}
                  aria-pressed={active}
                  onClick={() => setFilter(f.value)}
                >
                  {f.label}
                  {active && <small>{count}</small>}
                </button>
              );
            })}
          </div>
          <div className="studio-result-count" id="catalogCount" aria-live="polite">{filtered.length}개의 발매</div>
          {filtered.length ? (
            <div className="aq-catalog-cards">
              {filtered.map(r => (
                <article
                  key={r.id} className="aq-release-card"
                  onClick={() => openRelease(r.id)}
                  onKeyDown={e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); openRelease(r.id); } }}
                  tabIndex={0} role="button" aria-label={`${r.title || '제목 없는 발매'} 상세 보기`}
                >
                  <Cover id={r.id} />
                  <div className="min-0">
                    <span className="row-name">{r.title || '제목 없는 발매'}</span>
                    <span className="row-sub">{r.artist || '아티스트 미입력'} · {r.track_count}곡</span>
                    <span className="row-sub">{r.release_date ?? '발매일 미정'}</span>
                  </div>
                  <div className="aq-release-end">
                    <span className={`status-chip ${chipOf(r.status)}`}>{labelOf(r.status)}</span>
                  </div>
                </article>
              ))}
            </div>
          ) : (
            <div className="empty-page">
              <h2>{query ? '검색 결과가 없어요.' : '아직 등록한 발매가 없어요.'}</h2>
              <p>발매 정보를 저장하면 이곳에서 수정하고 관리할 수 있어요.</p>
            </div>
          )}
        </div>
      ) : (
        <div>
          <div className="toolbar">
            <input
              id="trackSearch" type="search" aria-label="트랙 검색" placeholder="곡명·아티스트·ISRC 검색"
              value={trackQuery} onChange={e => setTrackQuery(e.target.value)}
            />
          </div>
          <div className="studio-result-count" aria-live="polite">{filteredTracks.length}개의 트랙</div>
          {filteredTracks.length ? (
            <div className="aq-catalog-cards">
              {filteredTracks.map(t => (
                <div
                  key={t.id} className="track-row aq-track-row"
                  onClick={() => openRelease(t.releaseId)}
                  onKeyDown={e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); openRelease(t.releaseId); } }}
                  tabIndex={0} role="button" aria-label={`${t.title || '제목 없는 곡'} 상세 보기`}
                >
                  <Cover id={t.releaseId + t.id} />
                  <div className="min-0">
                    <span className="row-name">{t.title || '제목 없는 곡'}</span>
                    <span className="row-sub">{t.releaseTitle} · {t.artist || '아티스트 미입력'}{t.isrc ? ` · ISRC ${t.isrc}` : ''}</span>
                  </div>
                  <span className="chevron" aria-hidden="true">›</span>
                </div>
              ))}
            </div>
          ) : (
            <div className="empty-page">
              <h2>표시할 트랙이 없어요.</h2>
              <p>새로운 발매를 등록하면 곡별 정보를 볼 수 있어요.</p>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
