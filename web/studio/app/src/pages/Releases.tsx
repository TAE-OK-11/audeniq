import { useEffect, useState } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { mockApi } from '../api/mock';
import type { Release, Track } from '../api/client';

// mock 상태를 라이브 필터 카테고리로 매핑
const STATUS_MAP: Record<string, { chip: string; label: string }> = {
  LIVE: { chip: 'live', label: '발매 완료' },
  STAGE1_PASSED: { chip: 'review', label: '심사 통과' },
  STAGE1_CORRECTION: { chip: 'needs', label: '수정 필요' },
  DRAFT: { chip: 'draft', label: '초안' },
};

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

type TrackWithRelease = Track & { releaseTitle: string; releaseId: string };

function Cover({ title }: { title: string }) {
  return (
    <span className="cover" aria-hidden="true">
      {title.charAt(0)}
    </span>
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

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const rs = await mockApi.listReleases();
        if (cancelled) return;
        setList(rs);
        const all: TrackWithRelease[] = [];
        for (const r of rs) {
          try {
            const d = await mockApi.getRelease(r.id);
            if (cancelled) return;
            for (const t of d.tracks) {
              all.push({ ...t, releaseTitle: d.title, releaseId: d.id });
            }
          } catch { /* ignore */ }
        }
        setTracks(all);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => { cancelled = true; };
  }, []);

  const chipOf = (status: string) => STATUS_MAP[status]?.chip ?? 'draft';

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
    <>
      <div className="view-title">
        <div>
          <p className="eyebrow">CATALOG</p>
          <h1>내 음악</h1>
          <p>발매한 음악과 심사 진행 상황을 한눈에 확인해 보세요.</p>
        </div>
        <Link className="button" to="/upload">새 발매</Link>
      </div>

      <div className="tabs" role="tablist" aria-label="카탈로그 메뉴">
        <button
          type="button" className="tab" role="tab"
          aria-selected={tab === 'releases'}
          onClick={() => setTab('releases')}
        >발매 목록</button>
        <button
          type="button" className="tab" role="tab"
          aria-selected={tab === 'tracks'}
          onClick={() => setTab('tracks')}
        >전체 트랙</button>
      </div>

      {loading ? (
        <p style={{ color: 'var(--muted)' }}>불러오는 중...</p>
      ) : tab === 'releases' ? (
        <div>
          <div className="toolbar">
            <input
              type="search" aria-label="발매 검색" placeholder="발매명·아티스트 검색"
              value={query} onChange={e => setQuery(e.target.value)}
            />
            <select aria-label="발매 상태" value={filter} onChange={e => setFilter(e.target.value)}>
              {SELECT_OPTIONS.map(o => <option key={o.value} value={o.value}>{o.label}</option>)}
            </select>
          </div>
          <div className="studio-filterrail" role="group" aria-label="발매 진행 상태">
            {FILTERS.map(f => {
              const active = filter === f.value;
              const count = f.value === 'all'
                ? list.length
                : list.filter(r => chipOf(r.status) === f.value).length;
              return (
                <button
                  key={f.value} type="button"
                  className={`studio-filter${active ? ' active' : ''}`}
                  aria-pressed={active}
                  onClick={() => setFilter(f.value)}
                >
                  {f.label}
                  {active && <small>{count}</small>}
                </button>
              );
            })}
          </div>
          <div className="studio-result-count" aria-live="polite">{filtered.length}개의 발매</div>
          {filtered.length ? (
            <div className="data-list studio-catalog-list">
              {filtered.map(r => {
                const st = STATUS_MAP[r.status] ?? { chip: 'draft', label: r.status };
                return (
                  <article key={r.id} className="release-row studio-album-row">
                    <Cover title={r.title} />
                    <div className="min-0">
                      <button type="button" className="row-name" onClick={() => openRelease(r.id)}>
                        {r.title || '제목 없는 발매'}
                      </button>
                      <span className="row-sub">테스트 레이블 · {r.track_count}곡</span>
                      <span className="row-sub">{r.release_date ?? '발매일 미정'}</span>
                    </div>
                    <div className="row-end">
                      <span className={`status-chip ${st.chip}`}>{st.label}</span>
                    </div>
                  </article>
                );
              })}
            </div>
          ) : (
            <div className="empty-page">
              <h3>{query ? '검색 결과가 없어요.' : '아직 등록한 발매가 없어요.'}</h3>
              <p>발매 정보를 저장하면 이곳에서 수정하고 관리할 수 있어요.</p>
            </div>
          )}
        </div>
      ) : (
        <div>
          <div className="toolbar">
            <input
              type="search" aria-label="트랙 검색" placeholder="곡명·아티스트·ISRC 검색"
              value={trackQuery} onChange={e => setTrackQuery(e.target.value)}
            />
          </div>
          {filteredTracks.length ? (
            <div className="data-list">
              {filteredTracks.map(t => (
                <div key={t.id} className="track-row">
                  <Cover title={t.title} />
                  <div>
                    <span className="row-name">{t.title || '제목 없는 곡'}</span>
                    <span className="row-sub">{t.releaseTitle} · 테스트 레이블{t.isrc ? ` · ISRC ${t.isrc}` : ''}</span>
                  </div>
                  <button className="link-btn" type="button" onClick={() => openRelease(t.releaseId)}>
                    자세히 보기
                  </button>
                </div>
              ))}
            </div>
          ) : (
            <div className="empty-page">
              <h3>표시할 트랙이 없어요.</h3>
              <p>새로운 발매를 등록하면 곡별 정보를 볼 수 있어요.</p>
            </div>
          )}
        </div>
      )}
    </>
  );
}
