// 내 아티스트 페이지 — 아티스트별 스포티파이·애플뮤직·멜론 등 페이지 링크. 한 번 등록하면 다음 발매 신청에 자동으로 채워지고,
// 플랫폼에 보내는 발매 정보에 아티스트 ID로 들어가 같은 이름의 다른 아티스트 페이지로 잘못 올라가지 않게 한다.
// 첫 발매로 새 페이지가 만들어지는 중이면(대기) 페이지가 생긴 뒤 링크를 등록해 달라고 안내한다.
import { useCallback, useEffect, useState } from 'react';
import { orgPath, req } from '../api/http';
import { errorMessage } from '../api/errors';
import { MOCK } from '../lib/mode';
import { PAGE_PLATFORMS, type PageInfo } from '../lib/artistPages';
import { useToast } from './Toast';

interface ArtistRow { artist_id: string; name: string; pages: Record<string, PageInfo>; releases: number }

export function ArtistPagesCard() {
  const toast = useToast();
  const [items, setItems] = useState<ArtistRow[] | null>(null);
  const [edit, setEdit] = useState<string>('');
  const [links, setLinks] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const load = useCallback(async () => {
    if (MOCK) { setItems([]); return; }
    try { setItems((await req<{ items: ArtistRow[] }>(orgPath('/artist-pages/all'), { quiet401: true })).items.filter(a => a.releases > 0)); }
    catch { setItems(null); }
  }, []);
  useEffect(() => { void load(); }, [load]);
  if (!items?.length) return null;

  const open = (a: ArtistRow) => {
    setEdit(a.artist_id);
    setLinks(Object.fromEntries(PAGE_PLATFORMS.map(p => [p.key, a.pages[p.key]?.url ?? ''])));
  };
  const save = async (a: ArtistRow) => {
    setBusy(true);
    try {
      await req(orgPath(`/artists/${encodeURIComponent(a.artist_id)}/pages`), { method: 'PUT', body: { links } });
      toast('아티스트 페이지를 저장했어요. 다음 발매부터 자동으로 이 페이지에 올라가요.', 'success');
      setEdit(''); await load();
    } catch (e) { toast(errorMessage(e), 'error'); } finally { setBusy(false); }
  };

  return (
    <section className="ui-card aq-artist-pages" aria-labelledby="artistPagesHeading">
      <h2 id="artistPagesHeading" className="subhead">내 아티스트 페이지</h2>
      <p className="ui-field-help">한 번 등록하면 다음 발매 신청에 자동으로 채워지고, 플랫폼에 아티스트 ID로 전달돼 동명이인 페이지로 잘못 올라가지 않아요.</p>
      <ul className="aq-artist-pages-list">
        {items.map(a => {
          const pending = Object.entries(a.pages).filter(([, p]) => p.status === 'PENDING').map(([k]) => PAGE_PLATFORMS.find(p => p.key === k)?.label ?? k);
          const linked = Object.entries(a.pages).filter(([, p]) => p.status === 'LINKED');
          return (
            <li key={a.artist_id}>
              <div className="aq-artist-pages-head">
                <b>{a.name}</b>
                <span className="small muted">{linked.length ? linked.map(([k]) => PAGE_PLATFORMS.find(p => p.key === k)?.label ?? k).join(' · ') : '등록된 페이지 없음'}</span>
                {edit !== a.artist_id && <button type="button" className="ui-btn is-soft is-sm" onClick={() => open(a)}>{linked.length ? '수정' : '등록'}</button>}
              </div>
              {pending.length > 0 && edit !== a.artist_id && (
                <p className="small aq-artist-pages-pending">첫 발매로 {pending.join('·')}에 새 아티스트 페이지가 만들어져요. 페이지가 생기면 링크를 등록해 주세요.</p>
              )}
              {edit === a.artist_id && (
                <div className="aq-artist-pages-form">
                  {PAGE_PLATFORMS.map(p => (
                    <label key={p.key} className="aq-artist-pages-row">
                      <span>{p.label}</span>
                      <input className="ui-input" type="url" inputMode="url" maxLength={500} placeholder={p.placeholder}
                        value={links[p.key] ?? ''} onChange={e => setLinks(l => ({ ...l, [p.key]: e.target.value }))} />
                    </label>
                  ))}
                  <div className="aq-confirm-actions">
                    <button type="button" className="ui-btn is-soft" onClick={() => setEdit('')} disabled={busy}>닫기</button>
                    <button type="button" className="ui-btn is-primary" onClick={() => void save(a)} disabled={busy}>{busy ? '저장 중' : '저장'}</button>
                  </div>
                </div>
              )}
            </li>
          );
        })}
      </ul>
    </section>
  );
}
