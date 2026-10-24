// 아티스트 소개 페이지 편집 창 — 아티스트마다 하나의 공개 주소(/a/코드): 이름·소개 글·플랫폼 아티스트 페이지와
// 스마트링크를 켠 발매들. 계정 정보 > 아티스트 페이지(ArtistHubCard)의 '소개 페이지' 줄에서 열린다.
// 처음 저장하면 주소가 만들어지고, 꺼도 주소는 그대로라 다시 켜면 같은 링크로 열린다.
import { useEffect, useState } from 'react';
import { orgPath, req } from '../api/http';
import { errorMessage, messageForCode } from '../api/errors';
import { MOCK } from '../lib/mode';
import { num } from '../lib/format';
import { useProfile } from '../store/profile';
import { useModalClose } from './Modal';
import { Glyph } from './Glyph';
import { CopyButton } from './CopyButton';
import { QrButton } from './QrButton';
import { useToast } from './Toast';
import { BrandMark } from './BrandMark';
import { Badge } from '../ui';
import { gradientFor } from '../lib/catalog';
import { LINK_PRESETS, OWN_LABEL_MAX, OWN_LINKS_MAX, checkLinks, linkBrand, type OwnLink } from '../lib/ownLinks';
import { HANDLE_MAX, artistPath, artistPressUrl, artistUrl, handleProblem, normalizeHandle } from '../lib/publicLinks';

export interface ArtistLinkRow {
  artist_id: string; name: string; releases: number;
  link: { slug: string; enabled: boolean; bio: string; views: number; links?: OwnLink[]; contact?: string } | null;
}

const BIO_MAX = 1500;
const pageUrl = artistUrl;
const pressUrl = artistPressUrl;

/** 줄 오른쪽 글자 — 공개 중이면 조회 수, 꺼져 있으면 '꺼짐', 아직 없으면 '만들기' */
export function linkSummary(a: ArtistLinkRow): { text: string; empty: boolean } {
  if (!a.link) return { text: '만들기', empty: true };
  if (!a.link.enabled) return { text: '꺼짐', empty: false };
  return { text: `공개 중 · 최근 30일 ${num(a.link.views)}회`, empty: false };
}

export function LinkEditor({ artist, onSaved }: { artist: ArtistLinkRow; onSaved: (row: ArtistLinkRow) => void }) {
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
  // 주소 이름 — 아티스트가 고른다. 다른 아티스트가 쓰고 있으면 못 쓴다 (입력을 멈추면 서버에 물어본다)
  const [handle, setHandle] = useState(slug ?? '');
  const h = normalizeHandle(handle);
  const localProblem = h ? handleProblem(h) : null;
  const [remote, setRemote] = useState<{ handle: string; ok: boolean; reason?: string } | null>(null);
  useEffect(() => {
    if (!h || localProblem || h === slug) { setRemote(null); return; }
    let live = true;
    const t = window.setTimeout(() => {
      if (MOCK) { setRemote({ handle: h, ok: !['taken', 'press', 'audeniq'].includes(h), reason: 'ARTIST_LINK_HANDLE_TAKEN' }); return; }
      req<{ handle: string; available: boolean; reason?: string }>(
        orgPath(`/artists/${encodeURIComponent(artist.artist_id)}/link/handle?handle=${encodeURIComponent(h)}`), { quiet401: true })
        .then(r => { if (live) setRemote({ handle: r.handle, ok: r.available, reason: r.reason }); }, () => { if (live) setRemote(null); });
    }, 350);
    return () => { live = false; window.clearTimeout(t); };
  }, [h, localProblem, slug, artist.artist_id]);
  const checking = !!h && !localProblem && h !== slug && remote?.handle !== h;
  const handleMsg = localProblem
    ?? (remote && remote.handle === h && !remote.ok ? messageForCode(remote.reason ?? 'ARTIST_LINK_HANDLE_TAKEN') : null);

  const save = async (e: React.FormEvent) => {
    e.preventDefault();
    // 'https://'만 남은 줄은 빈 줄로 본다
    const checked = checkLinks(links.map(l => (l.url.trim() === 'https://' && !l.label.trim() ? { label: '', url: '' } : l)));
    if (checked.error) { setLinkError(checked.error); document.getElementById(`alLinkUrl${checked.error.index}`)?.focus(); return; }
    if (handleMsg) { document.getElementById('alHandle')?.focus(); return; }
    setBusy(true);
    try {
      const body = { enabled, bio, links: checked.links, contact: contact.trim(), ...(h && h !== slug ? { handle: h } : {}) };
      const r = MOCK
        ? { slug: h || slug || 'abcd2345', enabled }
        : await req<{ slug: string; enabled: boolean }>(orgPath(`/artists/${encodeURIComponent(artist.artist_id)}/link`), { method: 'PUT', body });
      onSaved({ ...artist, link: { slug: r.slug, enabled: r.enabled, bio: bio.trim(), views: artist.link?.views ?? 0, links: checked.links, contact: body.contact } });
      toast(!slug ? '아티스트 소개 페이지를 만들었어요. 링크를 SNS·프로필에 걸어 보세요.' : r.enabled ? '저장했어요.' : '페이지를 껐어요. 다시 켜면 같은 링크로 열려요.', 'success');
      close();
    } catch (err) { toast(errorMessage(err), 'error'); } finally { setBusy(false); }
  };

  const shown = slug ? pageUrl(slug).replace(/^https?:\/\//, '') : `${window.location.host}${artistPath(h || '…')}`;
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
            <a className="aq-promo-tool" href={artistPath(slug)} target="_blank" rel="noopener noreferrer"><Glyph name="arrow-up-right" size={14} />열기</a>
            <CopyButton text={pressUrl(slug)} label="보도자료" icon="doc" className="aq-promo-tool" />
          </div>
        ) : <p className="aq-al-note">저장하면 주소가 만들어져요. 꺼도 주소는 그대로예요.</p>}
      </div>

      <section className="aq-al-section" aria-labelledby="alHandleTitle">
        <div className="aq-al-section-head">
          <h3 id="alHandleTitle"><label htmlFor="alHandle">주소</label></h3>
          <span className="aq-al-count">{h.length} / {HANDLE_MAX}</span>
        </div>
        <input id="alHandle" className="ui-input" inputMode="url" autoCapitalize="none" autoCorrect="off" spellCheck={false} maxLength={HANDLE_MAX + 1}
          value={handle} onChange={e => setHandle(e.target.value)} onBlur={() => setHandle(h)} placeholder={slug ? undefined : '예: serin_official'}
          aria-invalid={!!handleMsg || undefined} aria-describedby="alHandleUrl alHandleHelp" />
        <small id="alHandleUrl" className="ui-field-help aq-al-handle-url">{window.location.host}/link/artist/<b>{h || '…'}</b></small>
        <small id="alHandleHelp" className={`ui-field-help${handleMsg ? ' aq-help-error' : ''}`} role={handleMsg ? 'alert' : undefined}>
          {handleMsg
            ?? (checking ? '확인하는 중…'
              : h && h !== slug && remote?.ok ? (slug ? '쓸 수 있어요. 바꾸면 예전 주소와 QR은 더 이상 열리지 않아요.' : '쓸 수 있어요.')
              : !h ? '비우면 임의의 주소가 만들어져요. 영문 소문자·숫자·_·- 3–30자.'
              : '영문 소문자·숫자·_·- 3–30자. 다른 아티스트가 쓰는 주소는 고를 수 없어요.')}
        </small>
      </section>

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
        <button type="submit" className={`ui-btn is-primary is-block${busy ? ' is-busy' : ''}`} disabled={busy || checking || !!handleMsg}>{busy ? '저장하는 중' : slug ? '저장' : '페이지 만들기'}</button>
      </div>
    </form>
  );
}
