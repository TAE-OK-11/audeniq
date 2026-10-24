// 플랫폼 아티스트 페이지 편집 창 — 아티스트별 스포티파이·애플뮤직·멜론 등 페이지 링크. 계정 정보 > 아티스트 페이지
// (ArtistHubCard)의 '플랫폼 페이지' 줄에서 열린다. 한 번 등록하면 다음 발매 신청에 자동으로 채워지고,
// 플랫폼에 아티스트 ID로 전달돼 동명이인 페이지로 잘못 올라가지 않는다.
import { useState } from 'react';
import { orgPath, req } from '../api/http';
import { errorMessage } from '../api/errors';
import { PAGE_PLATFORMS, type PageInfo } from '../lib/artistPages';
import { useModalClose } from './Modal';
import { useToast } from './Toast';

export interface ArtistRow { artist_id: string; name: string; pages: Record<string, PageInfo>; releases: number }

export const label = (k: string) => PAGE_PLATFORMS.find(p => p.key === k)?.label ?? k;

export function PagesEditor({ artist, onSaved }: { artist: ArtistRow; onSaved: () => void }) {
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
      toast('아티스트 페이지를 저장했어요. 다음 발매부터 자동으로 이 페이지에 올라가요.', 'success');
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

/** 줄 오른쪽 글자 — 연결된 플랫폼, 만들어지는 중, 또는 등록하기 */
export function pagesSummary(a: ArtistRow): { text: string; empty: boolean } {
  const linked = Object.entries(a.pages).filter(([, p]) => p.status === 'LINKED').map(([k]) => label(k));
  if (linked.length) return { text: linked.join(' · '), empty: false };
  return { text: Object.values(a.pages).some(p => p.status === 'PENDING') ? '페이지 생성 중' : '등록하기', empty: true };
}
