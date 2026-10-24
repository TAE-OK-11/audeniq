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
import { QrButton } from './QrButton';
import { useToast } from './Toast';
import { BrandMark } from './BrandMark';
import { Badge } from '../ui';
import { gradientFor } from '../lib/catalog';
import { LINK_PRESETS, OWN_LABEL_MAX, OWN_LINKS_MAX, checkLinks, linkBrand, type OwnLink } from '../lib/ownLinks';

export interface ArtistLinkRow {
  artist_id: string; name: string; releases: number;
  link: { slug: string; enabled: boolean; bio: string; views: number; links?: OwnLink[]; contact?: string } | null;
}

const BIO_MAX = 1500;
const pageUrl = (slug: string) => `${window.location.origin}/a/${slug}`;
const pressUrl = (slug: string) => `${window.location.origin}/a/${slug}/press`;

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
  const [links, setLinks] = useState<OwnLink[]>(() => artist.link?.links?.length ? artist.link.links : []);
  const [contact, setContact] = useState(artist.link?.contact ?? '');
  const [linkError, setLinkError] = useState<{ index: number; message: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const setLink = (i: number, patch: Partial<OwnLink>) => { setLinks(ls => ls.map((l, j) => (j === i ? { ...l, ...patch } : l))); setLinkError(null); };
  const addLink = (label = '') => setLinks(ls => (ls.length >= OWN_LINKS_MAX ? ls : [...ls, { label, url: 'https://' }]));
  const slug = artist.link?.slug;

  const save = async (e: React.FormEvent) => {
    e.preventDefault();
    // 'https://'만 남은 줄은 빈 줄로 본다
    const checked = checkLinks(links.map(l => (l.url.trim() === 'https://' && !l.label.trim() ? { label: '', url: '' } : l)));
    if (checked.error) { setLinkError(checked.error); document.getElementById(`alLinkUrl${checked.error.index}`)?.focus(); return; }
    setBusy(true);
    try {
      const body = { enabled, bio, links: checked.links, contact: contact.trim() };
      const r = MOCK
        ? { slug: slug ?? 'abcd2345', enabled }
        : await req<{ slug: string; enabled: boolean }>(orgPath(`/artists/${encodeURIComponent(artist.artist_id)}/link`), { method: 'PUT', body });
      onSaved({ ...artist, link: { slug: r.slug, enabled: r.enabled, bio: bio.trim(), views: artist.link?.views ?? 0, links: checked.links, contact: body.contact } });
      toast(!slug ? '아티스트 소개 페이지를 만들었어요. 링크를 SNS·프로필에 걸어 보세요.' : r.enabled ? '저장했어요.' : '페이지를 껐어요. 다시 켜면 같은 링크로 열려요.', 'success');
      close();
    } catch (err) { toast(errorMessage(err), 'error'); } finally { setBusy(false); }
  };

  const shown = slug ? pageUrl(slug).replace(/^https?:\/\//, '') : `${window.location.host}/a/…`;
  return (
    <form className="aq-set-editor aq-artist-link-editor" onSubmit={save} noValidate>
      {/* 맨 위 — 팬이 보게 될 페이지 한 장: 이름·상태·주소, 그 아래 공유 도구 */}
      <div className="aq-al-share">
        <div className="aq-al-head">
          <span className="aq-al-avatar" style={{ background: gradientFor(artist.artist_id) }} aria-hidden="true">{artist.name.trim().charAt(0)}</span>
          <span className="min-0">
            <strong>{artist.name}</strong>
            <span className="aq-al-url">{shown}</span>
          </span>
          {slug ? <Badge tone={enabled ? 'green' : 'gray'}>{enabled ? '공개 중' : '꺼짐'}</Badge> : <Badge tone="blue">새로 만들기</Badge>}
        </div>
        {slug ? (
          <div className="aq-promo-tools aq-al-tools" role="group" aria-label="공유">
            <CopyButton text={pageUrl(slug)} label="주소 복사" icon="link" className="aq-promo-tool" />
            <QrButton url={pageUrl(slug)} title={`${artist.name} 소개 페이지 QR 코드`} filename={`artist-${slug}.png`} className="aq-promo-tool" />
            <a className="aq-promo-tool" href={`/a/${slug}`} target="_blank" rel="noopener noreferrer"><Glyph name="arrow-up-right" size={14} />열기</a>
            <CopyButton text={pressUrl(slug)} label="보도자료" icon="doc" className="aq-promo-tool" />
          </div>
        ) : <p className="aq-al-note">저장하면 주소가 만들어져요. 꺼도 주소는 그대로예요.</p>}
      </div>

      <section className="aq-al-section" aria-labelledby="alBioTitle">
        <div className="aq-al-section-head">
          <h3 id="alBioTitle"><label htmlFor="alBio">소개 글</label></h3>
          <span className="aq-al-count">{bio.length.toLocaleString()} / {BIO_MAX.toLocaleString()}</span>
        </div>
        <textarea id="alBio" className="ui-input aq-al-bio" rows={4} maxLength={BIO_MAX} value={bio} onChange={e => setBio(e.target.value)}
          placeholder="어떤 음악을 만드는지, 팬에게 하고 싶은 말을 적어 주세요." />
      </section>

      {/* 링크 모음 — 인스타·유튜브·공연 예매처럼 직접 넣는 링크. 페이지에 넣은 순서대로 보인다 */}
      <section className="aq-al-section" aria-labelledby="alLinksTitle">
        <div className="aq-al-section-head">
          <h3 id="alLinksTitle">링크</h3>
          <span className="aq-al-count">{links.length} / {OWN_LINKS_MAX}</span>
        </div>
        {links.length > 0 && (
          <ul className="aq-al-links">
            {links.map((l, i) => {
              const brand = linkBrand(l.url);
              return (
                <li key={i} className={linkError?.index === i ? 'is-error' : undefined}>
                  {brand ? <BrandMark id={brand} label={l.label || brand} size={36} />
                    : <span className="aq-al-link-icon" aria-hidden="true"><Glyph name="link" size={16} /></span>}
                  <span className="aq-al-link-fields">
                    <input className="aq-al-link-label" aria-label={`링크 ${i + 1} 이름`} maxLength={OWN_LABEL_MAX} placeholder="이름 (예: 인스타그램)"
                      value={l.label} onChange={e => setLink(i, { label: e.target.value })} />
                    <input id={`alLinkUrl${i}`} className="aq-al-link-url" type="url" inputMode="url" aria-label={`링크 ${i + 1} 주소`} maxLength={500}
                      aria-invalid={linkError?.index === i || undefined} placeholder="https://"
                      value={l.url} onChange={e => setLink(i, { url: e.target.value })} />
                  </span>
                  <button type="button" className="aq-al-link-del" aria-label={`링크 ${i + 1} 삭제`}
                    onClick={() => { setLinks(ls => ls.filter((_, j) => j !== i)); setLinkError(null); }}><Glyph name="close" size={16} /></button>
                </li>
              );
            })}
          </ul>
        )}
        {linkError && <p className="ui-field-help aq-help-error" role="alert">{linkError.index + 1}번째 링크 · {linkError.message}</p>}
        {links.length < OWN_LINKS_MAX && (
          <div className="ui-chips is-wrap aq-al-chips" role="group" aria-label="링크 추가">
            {LINK_PRESETS.filter(p => !links.some(l => l.label.trim() === p)).map(p => (
              <button key={p} type="button" className="ui-chip is-sm" onClick={() => addLink(p)}><Glyph name="plus" size={12} />{p}</button>
            ))}
            <button type="button" className="ui-chip is-sm" onClick={() => addLink()}><Glyph name="plus" size={12} />직접 추가</button>
          </div>
        )}
      </section>

      <section className="aq-al-section" aria-labelledby="alContactTitle">
        <div className="aq-al-section-head"><h3 id="alContactTitle"><label htmlFor="alContact">문의 메일</label></h3></div>
        <input id="alContact" className="ui-input" type="email" inputMode="email" maxLength={254} value={contact}
          onChange={e => setContact(e.target.value)} placeholder="booking@example.com" />
        <small className="ui-field-help">보도자료 페이지에 ‘섭외·문의 메일 보내기’로 보여요. 비우면 숨겨요.</small>
      </section>

      <div className="aq-alert-pref aq-al-public">
        <span className="aq-alert-pref-icon" aria-hidden="true"><Glyph name="globe" size={18} /></span>
        <span className="min-0">
          <strong>누구나 볼 수 있게 공개</strong>
          <span>{artist.releases > 0 ? `스마트링크를 켠 발매 ${artist.releases}개가 보여요.` : '스마트링크를 켠 발매가 생기면 여기에 보여요. 발매 상세 > 관리에서 만들 수 있어요.'}</span>
        </span>
        <label className="aq-switch">
          <input type="checkbox" role="switch" aria-label="아티스트 소개 페이지 공개" checked={enabled} onChange={e => setEnabled(e.target.checked)} />
          <span aria-hidden="true" />
        </label>
      </div>
      <div className="aq-al-submit">
        <button type="submit" className={`ui-btn is-primary is-block${busy ? ' is-busy' : ''}`} disabled={busy}>{busy ? '저장하는 중' : slug ? '저장' : '페이지 만들기'}</button>
      </div>
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
