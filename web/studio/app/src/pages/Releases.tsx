import { useDeferredValue, useEffect, useMemo, useState } from 'react';
import { releaseDay } from '../lib/albumPrep';
import { useNavigate, useSearchParams } from '../lib/router';
import { api, type Release, type Track } from '../api/client';
import { prefetchRelease } from '../api/prefetch';
import { fixPath, isDocumentFix, resolveCorrection, splitCorrections } from '../lib/corrections';
import { STATUS_LABEL, statusTone } from '../lib/format';
import { useAsync } from '../hooks/useAsync';
import { SkeletonRows } from '../components/Skeleton';
import { ReleaseCover } from '../components/ReleaseCover';
import { Glyph } from '../components/Glyph';
import { Badge, Tabs } from '../ui';
import { downloadCodesCsv } from '../api/portal';
import { errorMessage } from '../api/errors';
import { useToast } from '../components/Toast';
import { MOCK } from '../lib/mode';
import { openFixes, pendingProofs, proofPath, useDocs, type DocRecord } from '../store/docs';
import { WorkLibrary } from '../components/publishing/WorkLibrary';
import { RegBadge } from '../components/publishing/parts';
import { pubApi } from '../api/publishing';

const FILTERS = [
  { value: 'all', label: '전체' },
  { value: 'draft', label: '작성 중' },
  { value: 'ready', label: '접수 대기' },
  { value: 'review', label: '검토 중' },
  { value: 'needs', label: '보완 필요' },
  { value: 'scheduled', label: '배급 승인' },
  { value: 'live', label: '발매 완료' },
  { value: 'rejected', label: '발매 거절' },
  { value: 'cancelled', label: '신청 취소' },
  { value: 'closed', label: '진행 종료' },
];

const SORTS = [
  { value: 'updated', label: '최근 수정순' },
  { value: 'date', label: '발매일순' },
  { value: 'title', label: '이름순' },
];

type TrackWithRelease = Track & { releaseTitle: string; releaseId: string; artist?: string; coverData?: string };

/** 첫 보완 요청 한 줄 요약 */
function firstFixText(r: Release, proofs: DocRecord[] = []): string {
  if (r.fixKind === 'documents') {
    if (!proofs.length) return '권리·보완 서류에서 담당자가 요청한 서류를 확인해 주세요.';
    return `요청 서류: ${proofs[0].title}${proofs.length > 1 ? ` 외 ${proofs.length - 1}건` : ''}`;
  }
  const { items: list, note } = splitCorrections(r.corrections);
  if (!list.length) return note ? `담당자 의견: ${note}` : '요청 내용을 확인하고 고쳐 주세요.';
  const first = resolveCorrection(list[0]);
  const text = first.message.startsWith(first.label) ? first.message : `${first.label}: ${first.message}`;
  return `${text}${list.length > 1 ? ` 외 ${list.length - 1}건` : ''}`;
}

const norm = (s: string | null | undefined) => (s ?? '').toLowerCase().replace(/\s+/g, '');

export function Releases() {
  const nav = useNavigate();
  const docs = useDocs();
  const [params, setParams] = useSearchParams();
  const tabParam = params.get('tab');
  const tab = tabParam === 'tracks' || tabParam === 'works' ? tabParam : 'releases';
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

  const { data: listData, loading: listLoading, error: listError, reload } = useAsync(() => api.listReleases(), [], { key: 'releases' });
  // 저작물 탭 개수 — 작품 목록은 라이브러리 탭과 같은 캐시를 쓴다 (퍼블리싱 API가 없는 서버면 빈 목록)
  const { data: works } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  const trackWork = useMemo(() => new Map((works ?? []).flatMap(w => w.recordings.map(r => [r.trackId, w] as const))), [works]);
  const list = useMemo(() => listData ?? [], [listData]);
  // 트랙은 '전체 트랙' 탭을 처음 열 때만 발매별 상세를 불러온다 (목록만 볼 때 N건 추가 요청 방지).
  // 탭에 손가락이 닿는 순간(누르기 직전) 미리 받기 시작해, 바꿨을 때 빈 화면이 거의 보이지 않게 한다
  const [tracksWanted, setTracksWanted] = useState(tab === 'tracks');
  useEffect(() => { if (tab === 'tracks') setTracksWanted(true); }, [tab]);
  const { data: trackData, loading: tracksLoading, error: tracksError } = useAsync(async () => {
    if (!tracksWanted || !listData) return null;
    const details = await Promise.all(listData.map(r => api.getRelease(r.id).catch(() => null)));
    const out: TrackWithRelease[] = [];
    for (const d of details) {
      if (!d) continue;
      for (const t of d.tracks) {
        out.push({ ...t, releaseTitle: d.title, releaseId: d.id, artist: d.artist, coverData: d.draft?.coverData });
      }
    }
    return out;
  }, [tracksWanted, listData], { key: tracksWanted && listData ? 'release-tracks' : undefined });
  const tracks = useMemo(() => trackData ?? [], [trackData]);
  const toast = useToast();
  const [csvBusy, setCsvBusy] = useState(false);
  const codesCsv = async () => {
    setCsvBusy(true);
    try { await downloadCodesCsv(); } catch (e) { toast(errorMessage(e, 'CSV를 받지 못했어요.'), 'error'); } finally { setCsvBusy(false); }
  };
  const trackCount = useMemo(() => list.reduce((n, r) => n + (r.track_count || 0), 0), [list]);
  const tracksPending = tab === 'tracks' && (tracksLoading || (!trackData && !tracksError));
  // 탭을 바꾼 뒤 트랙이 금방 오면 스켈레톤을 그리지 않는다 — 회색 줄이 한 번 번쩍였다가 바뀌어 튀어 보였다
  const [slowTracks, setSlowTracks] = useState(false);
  useEffect(() => {
    if (!tracksPending) { setSlowTracks(false); return; }
    const t = window.setTimeout(() => setSlowTracks(true), 300);
    return () => window.clearTimeout(t);
  }, [tracksPending]);
  const loading = listLoading || tracksPending;
  const error = listError || (tab === 'tracks' ? tracksError : '');


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
  // 곡을 누르면 그 발매 상세로 가서 곡 상세 창(ISRC·크레딧·가사)을 바로 연다
  const openTrack = (id: string, track: string) => nav(`/releases/${id}?track=${encodeURIComponent(track)}`);
  const onTrackKey = (id: string, track: string) => (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); openTrack(id, track); }
  };
  const onCardKey = (id: string) => (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); openRelease(id); }
  };

  return (
    <div id="view-catalog" className="view">
      <div className="ui-page-head aq-head-inline">
        <div>
          <p className="eyebrow">작업실</p>
          <h1 id="catalogTitle">발매·곡 관리</h1>
          <p>{tab === 'works' ? '작사·작곡한 작품의 지분과 저작권 관리 상태를 보여 드려요.' : '발매마다 심사가 어디까지 왔는지 보여 드려요.'}</p>
        </div>
        <button type="button" className="ui-btn is-primary" onClick={() => nav('/upload')}><Glyph name="plus" size={14} />새 발매</button>
      </div>

      <Tabs
        label="카탈로그 메뉴" value={tab}
        items={[
          { value: 'releases', label: '발매 목록', count: listLoading ? null : list.length },
          { value: 'tracks', label: '전체 트랙', count: listLoading ? null : trackCount },
          { value: 'works', label: '저작물', count: works?.length ?? null },
        ]}
        onChange={v => setParam('tab', v, 'releases')}
        onIntent={v => { if (v === 'tracks') setTracksWanted(true); if (v === 'works') void pubApi.listWorks(); }}
        className="is-fill"
      />

      {tab === 'works' ? (
        <WorkLibrary />
      ) : loading ? (
        listLoading || slowTracks ? <SkeletonRows count={4} /> : <div className="aq-tab-wait" aria-busy="true" />
      ) : error ? (
        <div className="ui-empty">
          <h2>발매 목록을 불러오지 못했어요.</h2>
          <p>{error}</p>
          <button type="button" className="ui-btn is-soft" onClick={reload}>다시 불러오기</button>
        </div>
      ) : tab === 'releases' ? (
        <div className="ui-tab-panel" key="releases">
          <div className="toolbar">
            <input
              id="catalogSearch" className="ui-input" type="search" aria-label="발매 검색" placeholder="발매·아티스트 검색"
              value={query} onChange={e => setQuery(e.target.value)}
            />
            <select id="catalogSort" className="ui-select" aria-label="정렬" value={sort} onChange={e => setSort(e.target.value)}>
              {SORTS.map(o => <option key={o.value} value={o.value}>{o.label}</option>)}
            </select>
          </div>
          <div className="ui-chips" id="catalogFilterRail" role="group" aria-label="발매 진행 상태">
            {FILTERS.map(f => {
              const active = filter === f.value;
              const count = counts[f.value] || 0;
              if (f.value !== 'all' && !count && !active) return null;
              return (
                <button
                  key={f.value} type="button"
                  className={`ui-chip${active ? ' is-on' : ''}`}
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
                // 카드 전체가 상세로 가는 링크 — article에는 link 역할을 줄 수 없어 div로(모양은 같음)
                <div
                  key={r.id} className="ui-row aq-release-card"
                  onPointerDown={() => prefetchRelease(r.id)}
                  onClick={() => openRelease(r.id)} onKeyDown={onCardKey(r.id)}
                  tabIndex={0} role="link" aria-label={`${r.title || '제목 없는 발매'} 상세 보기`}
                >
                  <ReleaseCover id={r.id} src={r.coverData} />
                  <div className="min-0">
                    <span className="ui-row-title">{r.title || '제목 없는 발매'}</span>
                    <span className="ui-row-meta">{r.artist || '아티스트 없음'} · {r.track_count}곡</span>
                    <span className="ui-row-meta">
                      {r.release_date ?? '발매일 아직 없음'}
                      {(() => { const dd = releaseDay(r.release_date, r.status); return dd && <b className={`aq-card-dday${dd.soon ? ' is-soon' : ''}`}>{dd.text}</b>; })()}
                    </span>
                  </div>
                  <div className="aq-release-end">
                    <Badge tone={statusTone(r.status)} className={`aq-status is-${r.status}`}>{STATUS_LABEL[r.status] || r.status}</Badge>
                  </div>
                  {r.status === 'needs' && (() => {
                    // 서류 요청은 신청서가 아니라 그 요청의 첨부·제출 화면으로 간다
                    const all = splitCorrections(r.corrections).items;
                    // 보완 항목으로 요청한 서류는 항목 쪽에서 안내하므로 따로 세지 않는다. 낸 서류 항목은 끝난 것으로
                    const linked = new Set(all.map(c => c.documentId).filter(Boolean));
                    const items = openFixes(all, docs);
                    const proofs = pendingProofs(docs, r.id, r.title).filter(d => !linked.has(d.id));
                    const docsOnly = r.fixKind === 'documents';
                    const count = docsOnly ? proofs.length : items.length;
                    const first = items[0];
                    return (
                      <div className="aq-release-fix">
                        <span className="min-0">
                          <b>{docsOnly ? '서류 제출 요청' : '보완 요청'}{count ? ` ${count}건` : ''}</b>
                          {!docsOnly && all.length > 0 && !items.length ? '요청한 서류를 냈어요. 신청서를 다시 접수해 주세요.' : firstFixText({ ...r, corrections: items }, proofs)}
                        </span>
                        <button
                          type="button" className="aq-release-fix-btn"
                          onClick={e => { e.stopPropagation(); nav(docsOnly ? proofPath(proofs[0]) : fixPath(r.id, first)); }}
                          onKeyDown={e => e.stopPropagation()}
                        >{docsOnly ? '서류 제출하기' : isDocumentFix(first) ? '서류 첨부하기' : '보완하기'} <Glyph name="chevron-right" size={14} /></button>
                        {!docsOnly && proofs.length > 0 && (
                          <button
                            type="button" className="aq-release-fix-btn"
                            onClick={e => { e.stopPropagation(); nav(proofPath(proofs[0])); }}
                            onKeyDown={e => e.stopPropagation()}
                          >서류 제출 {proofs.length}건 <Glyph name="chevron-right" size={14} /></button>
                        )}
                      </div>
                    );
                  })()}
                </div>
              ))}
            </div>
          ) : (
            <div className="ui-empty">
              <h2>{query || filter !== 'all' ? '조건에 맞는 발매가 없어요.' : '아직 등록한 발매가 없어요.'}</h2>
              <p>{query || filter !== 'all' ? '검색어나 상태 필터를 바꿔 보세요.' : '발매 정보를 저장하면 이곳에서 수정하고 관리할 수 있어요.'}</p>
              {(query || filter !== 'all') ? (
                <button type="button" className="ui-btn is-soft" onClick={() => { setQuery(''); setParam('status', 'all', 'all'); }}>필터 초기화</button>
              ) : (
                <button type="button" className="ui-btn is-primary" onClick={() => nav('/upload')}>첫 발매 등록하기</button>
              )}
            </div>
          )}
        </div>
      ) : (
        <div className="ui-tab-panel" key="tracks">
          <div className="toolbar">
            <input
              id="trackSearch" className="ui-input" type="search" aria-label="트랙 검색" placeholder="곡명·아티스트·ISRC 검색"
              value={trackQuery} onChange={e => setTrackQuery(e.target.value)}
            />
            {/* 발매·곡·UPC·ISRC 전체를 스프레드시트로 (GET /codes.csv) */}
            {!MOCK && (
              <button type="button" className="ui-btn is-soft aq-codes-csv" onClick={() => void codesCsv()} disabled={csvBusy}>
                <Glyph name="download" size={15} />{csvBusy ? '받는 중' : 'UPC·ISRC CSV'}
              </button>
            )}
          </div>
          <div className="studio-result-count" aria-live="polite">{filteredTracks.length}개의 트랙</div>
          {filteredTracks.length ? (
            <div className="aq-catalog-cards aq-stagger">
              {filteredTracks.map(t => (
                <div
                  key={t.releaseId + t.id} className="track-row ui-row aq-track-row"
                  onPointerDown={() => prefetchRelease(t.releaseId)}
                  onClick={() => openTrack(t.releaseId, t.id)} onKeyDown={onTrackKey(t.releaseId, t.id)}
                  tabIndex={0} role="link" aria-label={`${t.title || '제목 없는 곡'} 상세 보기`}
                >
                  <ReleaseCover id={t.releaseId} src={t.coverData} />
                  <div className="min-0">
                    <span className="ui-row-title">{t.title || '제목 없는 곡'}{t.explicit && <em className="aq-explicit" title="Explicit">E</em>}</span>
                    <span className="ui-row-meta">{t.releaseTitle} · {t.artist || '아티스트 없음'}</span>
                  </div>
                  {/* 녹음과 같은 곡의 작사·작곡 저작권 상태 — 배급과 퍼블리싱을 한 줄에서 */}
                  {trackWork.get(t.id) ? <RegBadge status={trackWork.get(t.id)!.registration} /> : <span className="chevron" aria-hidden="true"><Glyph name="chevron-right" size={18} /></span>}
                </div>
              ))}
            </div>
          ) : (
            <div className="ui-empty">
              <h2>{trackQuery ? '검색 결과가 없어요.' : '표시할 트랙이 없어요.'}</h2>
              <p>{trackQuery ? '다른 검색어로 찾아보세요.' : '새로운 발매를 등록하면 곡별 정보를 볼 수 있어요.'}</p>
              {trackQuery && <button type="button" className="ui-btn is-soft" onClick={() => setTrackQuery('')}>검색어 지우기</button>}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
