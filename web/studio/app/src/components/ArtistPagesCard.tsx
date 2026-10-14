// 계정 정보 > 아티스트 페이지 — 아티스트별 스포티파이·애플뮤직·멜론 등 페이지 링크. 계정 정보의 다른 항목과 같은
// '줄 + ›' 모양이고, 줄을 누르면 그 아티스트의 링크만 고치는 창이 열린다. 한 번 등록하면 다음 발매 신청에 자동으로 채워지고,
// 플랫폼에 아티스트 ID로 전달돼 동명이인 페이지로 잘못 올라가지 않는다.
import { useCallback, useEffect, useState } from 'react';
import { orgPath, req } from '../api/http';
import { errorMessage } from '../api/errors';
import { MOCK } from '../lib/mode';
import { PAGE_PLATFORMS, type PageInfo } from '../lib/artistPages';
import { Modal, useModalClose } from './Modal';
import { Glyph } from './Glyph';
import { useToast } from './Toast';

interface ArtistRow { artist_id: string; name: string; pages: Record<string, PageInfo>; releases: number }

const label = (k: string) => PAGE_PLATFORMS.find(p => p.key === k)?.label ?? k;

function PagesEditor({ artist, onSaved }: { artist: ArtistRow; onSaved: () => void }) {
  const close = useModalClose();
  const toast = useToast();
  const [links, setLinks] = useState<Record<string, string>>(() =>
    Object.fromEntries(PAGE_PLATFORMS.map(p => [p.key, artist.pages[p.key]?.url ?? ''])));
  const [busy, setBusy] = useState(false);
  const pending = Object.entries(artist.pages).filter(([, p]) => p.status === 'PENDING').map(([k]) => label(k));

  const save = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    try {
      await req(orgPath(`/artists/${encodeURIComponent(artist.artist_id)}/pages`), { method: 'PUT', body: { links } });
      toast('저장했어요.', 'success');
      onSaved();
      close();
    } catch (err) { toast(errorMessage(err), 'error'); } finally { setBusy(false); }
  };

  return (
    <form className="aq-set-editor" onSubmit={save} noValidate>
      {pending.length > 0 && (
        <p className="ui-callout">첫 발매로 {pending.join('·')}에 새 페이지가 만들어지는 중이에요. 페이지가 생기면 링크를 넣어 주세요.</p>
      )}
      {PAGE_PLATFORMS.map(p => (
        <div key={p.key} className="ui-field">
          <label htmlFor={`ap-${p.key}`}>{p.label}</label>
          <input id={`ap-${p.key}`} className="ui-input" type="url" inputMode="url" maxLength={500} placeholder={p.placeholder}
            value={links[p.key] ?? ''} onChange={e => setLinks(l => ({ ...l, [p.key]: e.target.value }))} />
        </div>
      ))}
      <button type="submit" className={`ui-btn is-primary is-block${busy ? ' is-busy' : ''}`} disabled={busy}>{busy ? '저장하는 중' : '저장'}</button>
    </form>
  );
}

export function ArtistPagesCard() {
  const [items, setItems] = useState<ArtistRow[] | null>(null);
  const [edit, setEdit] = useState<ArtistRow | null>(null);
  const load = useCallback(async () => {
    if (MOCK) { setItems([]); return; }
    try { setItems((await req<{ items: ArtistRow[] }>(orgPath('/artist-pages/all'), { quiet401: true })).items.filter(a => a.releases > 0)); }
    catch { setItems(null); }
  }, []);
  useEffect(() => { void load(); }, [load]);
  if (!items?.length) return null;

  return (
    <section className="aq-set-card" aria-labelledby="setArtistPages">
      <h2 id="setArtistPages">아티스트 페이지</h2>
      {items.map(a => {
        const linked = Object.entries(a.pages).filter(([, p]) => p.status === 'LINKED').map(([k]) => label(k));
        const waiting = Object.values(a.pages).some(p => p.status === 'PENDING');
        return (
          <button key={a.artist_id} type="button" className="aq-set-row" onClick={() => setEdit(a)}>
            <span className="aq-set-label">{a.name}</span>
            <span className={`aq-set-value${linked.length ? '' : ' is-empty'}`}>
              {linked.length ? linked.join(' · ') : waiting ? '페이지 생성 중' : '등록하기'}
            </span>
            <Glyph name="chevron-right" size={16} className="aq-set-chev" />
          </button>
        );
      })}
      {edit && (
        <Modal title={edit.name} onClose={() => setEdit(null)} modalClass="aq-set-modal">
          <PagesEditor artist={edit} onSaved={() => void load()} />
        </Modal>
      )}
    </section>
  );
}
