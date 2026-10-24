// 계정 정보 > 아티스트 소개 페이지 — 아티스트마다 하나의 공개 주소(/a/코드): 이름·소개 글·플랫폼 아티스트 페이지와
// 스마트링크를 켠 발매들. 계정 정보의 다른 항목과 같은 '줄 + ›' 모양이고, 줄을 누르면 소개 글·공개 여부를 고치는 창이 열린다.
// 처음 저장하면 주소가 만들어지고, 꺼도 주소는 그대로라 다시 켜면 같은 링크로 열린다.
import { useCallback, useEffect, useState } from 'react';
import { orgPath, req } from '../api/http';
import { errorMessage } from '../api/errors';
import { MOCK } from '../lib/mode';
import { num } from '../lib/format';
import { useProfile } from '../store/profile';
import { Modal, useModalClose } from './Modal';
import { Glyph } from './Glyph';
import { CopyButton } from './CopyButton';
import { useToast } from './Toast';

export interface ArtistLinkRow {
  artist_id: string; name: string; releases: number;
  link: { slug: string; enabled: boolean; bio: string; views: number } | null;
}

const BIO_MAX = 1500;
const pageUrl = (slug: string) => `${window.location.origin}/a/${slug}`;

/** 줄 오른쪽 글자 — 공개 중이면 조회 수, 꺼져 있으면 '꺼짐', 아직 없으면 '만들기' */
export function linkSummary(a: ArtistLinkRow): { text: string; empty: boolean } {
  if (!a.link) return { text: '만들기', empty: true };
  if (!a.link.enabled) return { text: '꺼짐', empty: false };
  return { text: `공개 중 · 최근 30일 ${num(a.link.views)}회`, empty: false };
}

function LinkEditor({ artist, onSaved }: { artist: ArtistLinkRow; onSaved: (row: ArtistLinkRow) => void }) {
  const close = useModalClose();
  const toast = useToast();
  const profile = useProfile();
  // 처음 만들 때는 계정 정보의 아티스트 소개를 그대로 가져온다
  const [bio, setBio] = useState(artist.link?.bio ?? profile.bio ?? '');
  const [enabled, setEnabled] = useState(artist.link?.enabled ?? true);
  const [busy, setBusy] = useState(false);
  const slug = artist.link?.slug;

  const save = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    try {
      const r = MOCK
        ? { slug: slug ?? 'abcd2345', enabled }
        : await req<{ slug: string; enabled: boolean }>(orgPath(`/artists/${encodeURIComponent(artist.artist_id)}/link`), { method: 'PUT', body: { enabled, bio } });
      onSaved({ ...artist, link: { slug: r.slug, enabled: r.enabled, bio: bio.trim(), views: artist.link?.views ?? 0 } });
      toast(!slug ? '아티스트 소개 페이지를 만들었어요. 링크를 SNS·프로필에 걸어 보세요.' : r.enabled ? '저장했어요.' : '페이지를 껐어요. 다시 켜면 같은 링크로 열려요.', 'success');
      close();
    } catch (err) { toast(errorMessage(err), 'error'); } finally { setBusy(false); }
  };

  return (
    <form className="aq-set-editor aq-artist-link-editor" onSubmit={save} noValidate>
      {slug && (
        <div className="aq-smartlink-url">
          <code>{pageUrl(slug)}</code>
          <span className="ui-actions">
            <CopyButton text={pageUrl(slug)} />
            <a className="ui-btn is-sm" href={`/a/${slug}`} target="_blank" rel="noopener noreferrer"><Glyph name="arrow-up-right" size={14} />열기</a>
          </span>
        </div>
      )}
      <div className="ui-field">
        <label htmlFor="alBio">소개 글<span className="aq-optional">선택</span></label>
        <textarea id="alBio" rows={5} maxLength={BIO_MAX} value={bio} onChange={e => setBio(e.target.value)}
          placeholder="어떤 음악을 만드는지, 팬에게 하고 싶은 말을 적어 주세요." />
        <small className="ui-field-help">{bio.length} / {BIO_MAX}</small>
      </div>
      <div className="aq-alert-pref">
        <span className="aq-alert-pref-icon" aria-hidden="true"><Glyph name="globe" size={18} /></span>
        <span className="min-0">
          <strong>누구나 볼 수 있게 공개</strong>
          <span>{artist.releases > 0 ? `스마트링크를 켠 발매 ${artist.releases}개가 보여요. 발매를 누르면 스마트링크로 이동해요.` : '스마트링크를 켠 발매가 생기면 여기에 보여요. 발매 상세 > 관리에서 스마트링크를 만들 수 있어요.'}</span>
        </span>
        <label className="aq-switch">
          <input type="checkbox" role="switch" aria-label="아티스트 소개 페이지 공개" checked={enabled} onChange={e => setEnabled(e.target.checked)} />
          <span aria-hidden="true" />
        </label>
      </div>
      <button type="submit" className={`ui-btn is-primary is-block${busy ? ' is-busy' : ''}`} disabled={busy}>{busy ? '저장하는 중' : slug ? '저장' : '페이지 만들기'}</button>
    </form>
  );
}

export function ArtistLinkCard() {
  const profile = useProfile();
  const [items, setItems] = useState<ArtistLinkRow[] | null>(null);
  const [edit, setEdit] = useState<ArtistLinkRow | null>(null);
  const load = useCallback(async () => {
    // 체험 모드: 활동명으로 아티스트 하나 (저장은 이 화면 안에서만)
    if (MOCK) { setItems(cur => cur ?? [{ artist_id: 'mock', name: profile.name.trim() || '서린', releases: 2, link: null }]); return; }
    try { setItems((await req<{ items: ArtistLinkRow[] }>(orgPath('/artist-links'), { quiet401: true })).items); }
    catch { setItems(null); }
  }, [profile.name]);
  useEffect(() => { void load(); }, [load]);
  if (!items?.length) return null;

  return (
    <section className="aq-set-card" aria-labelledby="setArtistLink">
      <h2 id="setArtistLink">아티스트 소개 페이지</h2>
      <p className="ui-field-help aq-set-help">이름·소개 글과 발매를 한 주소로 모아 SNS·프로필에 걸 수 있어요. 발매를 누르면 그 발매의 스마트링크로 이동해요.</p>
      {items.map(a => {
        const s = linkSummary(a);
        return (
          <button key={a.artist_id} type="button" className="aq-set-row" onClick={() => setEdit(a)}>
            <span className="aq-set-label">{a.name}</span>
            <span className={`aq-set-value${s.empty ? ' is-empty' : ''}`}>{s.text}</span>
            <Glyph name="chevron-right" size={16} className="aq-set-chev" />
          </button>
        );
      })}
      {edit && (
        <Modal title={`${edit.name} 소개 페이지`} onClose={() => setEdit(null)} modalClass="aq-set-modal">
          <LinkEditor artist={edit} onSaved={row => setItems(list => list?.map(x => (x.artist_id === row.artist_id ? row : x)) ?? null)} />
        </Modal>
      )}
    </section>
  );
}
