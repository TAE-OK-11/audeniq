// 발매·곡 관리 '식별코드' 탭 — 접수한 발매의 UPC와 곡별 ISRC를 한곳에서 (GET /api/orgs/{org}/codes).
// 플랫폼·유통사·저작권 단체에 낼 때 바로 복사하고, 전체를 CSV로 받는다. 테스트 범위 코드는 실제 배급에 쓰이지 않아 따로 표시한다.
import { useDeferredValue, useMemo, useState } from 'react';
import { useNavigate } from '../lib/router';
import { api } from '../api/client';
import { downloadCodesCsv, fetchCodes, type CodeRelease } from '../api/portal';
import { errorMessage } from '../api/errors';
import { useAsync } from '../hooks/useAsync';
import { MOCK } from '../lib/mode';
import { Badge } from '../ui';
import { CopyButton } from './CopyButton';
import { Glyph } from './Glyph';
import { SkeletonRows } from './Skeleton';
import { useToast } from './Toast';

/** 체험 모드 — 예시 발매에서 같은 모양을 만든다 (작성 중은 서버처럼 빼고) */
async function mockCodes(): Promise<CodeRelease[]> {
  const list = (await api.listReleases()).filter(r => r.status !== 'draft');
  const details = await Promise.all(list.map(r => api.getRelease(r.id).catch(() => null)));
  return details.filter(d => !!d).map(d => ({
    releaseId: d.id, title: d.title, artist: d.artist ?? '', releaseDate: d.release_date ?? '', status: d.status,
    upc: d.draft?.upc ?? '', upcVirtual: false,
    tracks: d.tracks.map((t, i) => ({ trackId: t.id, title: t.title, disc: 1, number: i + 1, isrc: t.isrc ?? '', isrcVirtual: false })),
  }));
}

const norm = (s: string) => s.toLowerCase().replace(/[\s-]/g, '');

export function CodesPanel() {
  const nav = useNavigate();
  const toast = useToast();
  const { data, loading, error, reload } = useAsync(() => (MOCK ? mockCodes() : fetchCodes()), [], { key: 'codes' });
  const [query, setQuery] = useState('');
  const q = norm(useDeferredValue(query));
  const [busy, setBusy] = useState(false);
  const items = useMemo(() => {
    const all = data ?? [];
    if (!q) return all;
    return all.filter(r => norm(`${r.title}${r.artist}${r.upc}`).includes(q) || r.tracks.some(t => norm(`${t.title}${t.isrc}`).includes(q)));
  }, [data, q]);
  const counts = useMemo(() => {
    const all = data ?? [];
    const tracks = all.flatMap(r => r.tracks);
    return { upc: all.filter(r => r.upc).length, isrc: tracks.filter(t => t.isrc).length, tracks: tracks.length, test: all.some(r => r.upcVirtual || r.tracks.some(t => t.isrcVirtual)) };
  }, [data]);
  const csv = async () => {
    setBusy(true);
    try { await downloadCodesCsv(); } catch (e) { toast(errorMessage(e, 'CSV를 받지 못했어요.'), 'error'); } finally { setBusy(false); }
  };

  if (loading && !data) return <SkeletonRows count={3} />;
  if (error) {
    return (
      <div className="ui-empty">
        <h2>식별코드를 불러오지 못했어요.</h2><p>{error}</p>
        <button type="button" className="ui-btn is-soft" onClick={reload}>다시 불러오기</button>
      </div>
    );
  }
  return (
    <div className="aq-codes">
      <section className="aq-codes-summary" aria-label="발급 현황">
        <div><span>UPC</span><b>{counts.upc}<small>개 발매</small></b></div>
        <div><span>ISRC</span><b>{counts.isrc}<small>/{counts.tracks}곡</small></b></div>
        <button type="button" className="ui-btn is-soft" onClick={() => void csv()} disabled={busy || !data?.length}>
          <Glyph name="download" size={15} />{busy ? '받는 중' : 'CSV로 받기'}
        </button>
      </section>
      {counts.test && <p className="ui-callout">‘테스트 코드’는 연동 확인용 범위라 실제 플랫폼 배급에 쓰이지 않아요. 승인되면 실제 코드로 바뀌어요.</p>}
      <div className="toolbar">
        <input className="ui-input" type="search" aria-label="코드 검색" placeholder="발매·곡·UPC·ISRC 검색" value={query} onChange={e => setQuery(e.target.value)} />
      </div>
      {items.length === 0 ? (
        <div className="ui-empty">
          <h2>{query ? '검색 결과가 없어요.' : '아직 발급된 코드가 없어요.'}</h2>
          <p>{query ? '다른 검색어로 찾아보세요.' : '발매를 접수하고 승인되면 UPC와 ISRC가 여기에 모여요.'}</p>
        </div>
      ) : (
        <div className="aq-codes-list">
          {items.map(r => (
            <section key={r.releaseId} className="aq-relinfo-card aq-code-card" aria-label={`${r.title} 식별코드`}>
              <header className="aq-code-head">
                <button type="button" className="aq-code-title" onClick={() => nav(`/releases/${encodeURIComponent(r.releaseId)}`)}>
                  <b>{r.title || '제목 없는 발매'}</b>
                  <span>{[r.artist, r.releaseDate].filter(Boolean).join(' · ')}</span>
                </button>
                <div className="aq-code-upc">
                  <span className="aq-code-kind">UPC</span>
                  {r.upc ? <><span className="is-mono">{r.upc}</span>{r.upcVirtual && <Badge tone="amber">테스트 코드</Badge>}<CopyButton text={r.upc} className="ui-btn is-xs is-white" /></> : <span className="aq-empty-val">발급 전</span>}
                </div>
              </header>
              {r.tracks.length > 0 && (
                <ol className="aq-code-tracks">
                  {r.tracks.map(t => (
                    <li key={t.trackId}>
                      <span className="aq-trackrow-no">{t.number || ''}</span>
                      <span className="aq-code-track">{t.title || '곡명 없음'}</span>
                      {t.isrc ? (
                        <span className="aq-code-isrc"><span className="is-mono">{t.isrc}</span>{t.isrcVirtual && <Badge tone="amber">테스트</Badge>}<CopyButton text={t.isrc} className="ui-btn is-xs is-white" /></span>
                      ) : <span className="aq-empty-val">ISRC 발급 전</span>}
                    </li>
                  ))}
                </ol>
              )}
            </section>
          ))}
        </div>
      )}
    </div>
  );
}
