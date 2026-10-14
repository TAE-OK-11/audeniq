// 코드 관리 — 신청한 발매의 UPC와 곡별 ISRC를 한 표로. 시험용(가상) 코드는 표시하고, CSV로 내려받을 수 있다.
import { useCallback, useEffect, useState } from 'react';
import { downloadFile, orgPath, req } from '../api/http';
import { errorMessage } from '../api/errors';
import { MOCK } from '../lib/mode';
import { useToast } from './Toast';

interface CodeTrack { track_id: string; title: string; disc: number; number: number; isrc: string | null; isrc_virtual: boolean }
interface CodeRelease {
  release_id: string; title: string; artist: string | null; release_date: string | null; status: string;
  upc: string | null; upc_virtual: boolean; tracks: CodeTrack[];
}

function Code({ value, virtual }: { value: string | null; virtual: boolean }) {
  if (!value) return <span className="muted">발급 전</span>;
  return (
    <span className="aq-code">
      <code>{value}</code>
      {virtual && <span className="aq-code-tag" title="시험용 코드라 실제 플랫폼에는 쓰이지 않아요">시험용</span>}
    </span>
  );
}

export function CodesPanel() {
  const toast = useToast();
  const [items, setItems] = useState<CodeRelease[] | null>(null);
  const [error, setError] = useState('');
  const [q, setQ] = useState('');
  const load = useCallback(async () => {
    if (MOCK) { setItems([]); return; }
    setError('');
    try { setItems((await req<{ items: CodeRelease[] }>(orgPath('/codes'))).items); } catch (e) { setError(errorMessage(e)); }
  }, []);
  useEffect(() => { void load(); }, [load]);
  const copy = (v: string) => { void navigator.clipboard?.writeText(v).then(() => toast(`${v} 복사했어요.`, 'success')); };
  const download = async () => {
    try { await downloadFile(orgPath('/codes.csv'), 'codes.csv'); } catch (e) { toast(errorMessage(e), 'error'); }
  };

  if (error) {
    return (
      <div className="ui-empty"><h2>코드를 불러오지 못했어요.</h2><p>{error}</p>
        <button type="button" className="ui-btn is-soft" onClick={() => void load()}>다시 불러오기</button></div>
    );
  }
  if (!items) return <p className="muted">불러오는 중…</p>;
  const needle = q.trim().toLowerCase();
  const shown = needle
    ? items.filter(r => [r.title, r.artist ?? '', r.upc ?? '', ...r.tracks.flatMap(t => [t.title, t.isrc ?? ''])].some(v => v.toLowerCase().includes(needle)))
    : items;

  return (
    <div className="ui-tab-panel" key="codes">
      <div className="toolbar">
        <input className="ui-input" type="search" aria-label="코드 검색" placeholder="발매명·곡명·UPC·ISRC 검색" value={q} onChange={e => setQ(e.target.value)} />
        <button type="button" className="ui-btn is-soft" onClick={() => void download()} disabled={!items.length}>CSV 내려받기</button>
      </div>
      <p className="small muted">UPC는 앨범, ISRC는 곡마다 붙는 고유 코드예요. 저작권 협회 등록이나 유통 문의 때 쓰세요.</p>
      {shown.length === 0 ? (
        <div className="ui-empty"><h2>{items.length ? '검색 결과가 없어요' : '아직 코드가 없어요'}</h2>
          <p>{items.length ? '다른 검색어로 찾아 보세요.' : '발매를 신청하면 이곳에 UPC·ISRC가 모여요.'}</p></div>
      ) : (
        <div className="aq-codes">
          {shown.map(r => (
            <section key={r.release_id} className="ui-card aq-codes-release">
              <header>
                <div><b>{r.title}</b>{r.artist && <span className="muted"> · {r.artist}</span>}
                  {r.release_date && <span className="muted small"> · {r.release_date}</span>}</div>
                <div className="aq-codes-upc">UPC <Code value={r.upc} virtual={r.upc_virtual} />
                  {r.upc && <button type="button" className="ui-btn is-text is-sm" onClick={() => copy(r.upc!)}>복사</button>}</div>
              </header>
              <ol className="aq-codes-tracks">
                {r.tracks.map(t => (
                  <li key={t.track_id}>
                    <span className="aq-codes-no">{r.tracks.some(x => x.disc > 1) ? `${t.disc}-` : ''}{t.number}</span>
                    <span className="aq-codes-title">{t.title}</span>
                    <Code value={t.isrc} virtual={t.isrc_virtual} />
                    {t.isrc && <button type="button" className="ui-btn is-text is-sm" onClick={() => copy(t.isrc!)}>복사</button>}
                  </li>
                ))}
              </ol>
            </section>
          ))}
        </div>
      )}
    </div>
  );
}
