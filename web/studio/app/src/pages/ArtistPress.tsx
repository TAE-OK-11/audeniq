// 보도자료 페이지 (/link/artist/:slug/press) — 아티스트 소개 페이지의 변형. 플레이리스트 큐레이터·기자·공연 기획자에게 보내는 한 장:
// 한 줄 소개·사실(장르·활동 지역·소속·데뷔), 연락처(아티스트가 넣었을 때만), 보도용 소개(복사), 주요 활동·인용,
// 아티스트가 공개한 보도자료(전문 보기·복사), 프로필 사진 받기, 발매마다 스마트링크·커버 받기·홍보 이미지, 플랫폼 페이지와 링크.
// 소개 페이지와 같은 데이터(useArtistPage)에 보도자료(/press, crates/core/src/artist_press.rs)를 더한다. 소개 페이지를 끄면 이 페이지도 닫힌다.
import { useEffect, useState } from 'react';
import { Link, useParams } from '../lib/router';
import { API_BASE, req } from '../api/http';
import { MOCK } from '../lib/mode';
import { PLATFORM_NAMES } from '../lib/platformNames';
import { gradientFor } from '../lib/catalog';
import { linkBrand, shortUrl } from '../lib/ownLinks';
import { releasePath, releaseUrl } from '../lib/publicLinks';
import { BrandMark } from '../components/BrandMark';
import { Glyph } from '../components/Glyph';
import { PromoImageButton } from '../components/PromoImageButton';
import { CopyButton } from '../components/CopyButton';
import { MOCK_PUBLIC_PRESS, fullText, type PressProfile, type PublicPressRelease } from '../lib/artistPress';
import { RELEASE_TYPE, artistBackdrop, useArtistPage, type ArtistRelease } from './ArtistPage';
import { ArtistSubHead, PublicFoot, PublicLoading, PublicMissing, PublicShell } from '../components/PublicPage';

const coverUrl = (slug: string) => `${API_BASE}/api/smart-links/${encodeURIComponent(slug)}/cover?media=2`;
const dotted = (d: string | null) => (d ? d.replace(/-/g, '.') : '발매일 미정');
/** 사실 — 적은 것만 */
const facts = (p: PressProfile): [string, string][] =>
  ([['장르', p.genre], ['활동 지역', p.based_in], ['소속', p.label], ['데뷔', p.debut && `${p.debut}년`]] as [string, string][]).filter(([, v]) => !!v);

function PressRelease({ r, artist }: { r: ArtistRelease; artist: string }) {
  const [ok, setOk] = useState(true);
  const cover = r.cover && ok && !MOCK ? coverUrl(r.slug) : '';
  const link = releaseUrl(r.slug);
  return (
    <li className="aq-press-release">
      {cover
        ? <img className="aq-press-cover" src={cover} alt="" loading="lazy" onError={() => setOk(false)} />
        : <span className="aq-press-cover is-empty" style={{ background: gradientFor(r.slug) }} aria-hidden="true"><Glyph name="music" className="aq-cover-glyph" /></span>}
      <div className="min-0">
        <strong className="ui-row-title">{r.title}</strong>
        <span className="ui-row-meta"><span>{RELEASE_TYPE[r.type] ?? r.type}</span><span>{dotted(r.release_date)}{r.released ? '' : ' 발매 예정'}</span></span>
        <div className="aq-press-actions">
          <Link to={releasePath(r.slug)} className="ui-btn is-sm"><Glyph name="link" size={14} />스마트링크</Link>
          {cover && <a className="ui-btn is-sm" href={cover} download={`${r.title}-cover.jpg`} target="_blank" rel="noopener noreferrer"><Glyph name="download" size={14} />커버 받기</a>}
          <PromoImageButton input={{ title: r.title, artist, releaseDate: r.release_date, url: link, cover: cover || undefined }} filename={`audeniq-${r.slug}`} />
        </div>
      </div>
    </li>
  );
}

/** 공개한 보도자료 한 건 — 제목·날짜·첫 문단, 전문은 펼쳐 보고 복사한다 */
function PressReleaseCard({ r }: { r: PublicPressRelease }) {
  const [open, setOpen] = useState(false);
  const date = r.published_at ? r.published_at.slice(0, 10).replace(/-/g, '.') : '';
  return (
    <li className="aq-pr-card">
      <strong>{r.title}</strong>
      {date && <small>{date}</small>}
      {r.lead && <p className="aq-pr-lead">{r.lead}</p>}
      {r.body && (
        <div className={`aq-disco-rest${open ? ' is-open' : ''}`} inert={!open}>
          <p className="aq-pr-body">{r.body}</p>
        </div>
      )}
      <div className="aq-press-actions">
        {r.body && (
          <button type="button" className="ui-btn is-sm" aria-expanded={open} onClick={() => setOpen(o => !o)}>
            <Glyph name="chevron-down" size={14} />{open ? '접기' : '전문 보기'}
          </button>
        )}
        <CopyButton text={fullText(r)} label="전문 복사" icon="doc" className="ui-btn is-sm" />
        {r.release && <Link to={releasePath(r.release.slug)} className="ui-btn is-sm"><Glyph name="link" size={14} />스마트링크</Link>}
      </div>
    </li>
  );
}

export function ArtistPress() {
  const { slug = '' } = useParams<{ slug: string }>();
  const { page, missing } = useArtistPage(slug, '보도자료 · AUDENIQ');
  const [kit, setKit] = useState<{ profile: PressProfile; releases: PublicPressRelease[] } | null>(null);
  useEffect(() => {
    if (MOCK) { setKit(MOCK_PUBLIC_PRESS()); return; }
    req<{ profile: PressProfile; releases: PublicPressRelease[] }>(`/api/artist-links/${encodeURIComponent(slug)}/press`, { quiet401: true })
      .then(setKit, () => setKit(null));
  }, [slug]);

  if (missing) return <PublicMissing />;
  if (!page) return <PublicLoading />;
  const latest = page.releases[0];

  return (
    <PublicShell bg={artistBackdrop(page)} className="aq-artistpage aq-presspage" cardClass="aq-artist-card aq-artist-sub aq-press-card">
        {/* 머리 작은 글자에 요약까지 — 영어 대문자 라벨(PRESS KIT)은 쓰지 않는다 */}
        <ArtistSubHead slug={slug} name={page.name} photo={page.photo} version={page.photo_v}
          title={`보도자료 · 발매 ${page.releases.length}개${latest?.release_date ? ` · 최근 ${dotted(latest.release_date)}` : ''}`} />
        {kit?.profile.tagline && <p className="aq-press-tagline">{kit.profile.tagline}</p>}
        {kit && facts(kit.profile).length > 0 && (
          <dl className="aq-press-facts">
            {facts(kit.profile).map(([k, v]) => <div key={k}><dt>{k}</dt><dd>{v}</dd></div>)}
          </dl>
        )}
        {page.contact && (
          <div className="aq-press-contact">
            <a className="ui-btn is-primary is-block" href={`mailto:${page.contact}?subject=${encodeURIComponent(`[${page.name}] 문의`)}`}>
              <Glyph name="mail" size={16} className="aq-glyph-lead" />섭외·문의 메일 보내기
            </a>
            <small>{page.contact}</small>
          </div>
        )}
        {kit?.profile.bio_short && (
          <section aria-labelledby="pressShort">
            <div className="aq-press-head-row">
              <h2 id="pressShort" className="aq-smartlink-list-title">보도용 소개</h2>
              <CopyButton text={kit.profile.bio_short} label="복사" className="ui-btn is-sm" />
            </div>
            <p className="aq-artist-bio aq-press-bio">{kit.profile.bio_short}</p>
          </section>
        )}
        {page.bio && (
          <section aria-labelledby="pressBio">
            <h2 id="pressBio" className="aq-smartlink-list-title">{kit?.profile.bio_short ? '자세한 소개' : '소개'}</h2>
            <p className="aq-artist-bio aq-press-bio">{page.bio}</p>
          </section>
        )}
        {!!kit?.profile.highlights.length && (
          <section aria-labelledby="pressHighlights">
            <h2 id="pressHighlights" className="aq-smartlink-list-title">주요 활동</h2>
            <ul className="aq-press-highlights">{kit.profile.highlights.map((h, i) => <li key={i}>{h}</li>)}</ul>
          </section>
        )}
        {!!kit?.profile.quotes.length && (
          <section aria-labelledby="pressQuotes">
            <h2 id="pressQuotes" className="aq-smartlink-list-title">한마디</h2>
            {kit.profile.quotes.map((q, i) => (
              <figure key={i} className="aq-press-quote">
                <blockquote>{q.quote}</blockquote>
                <figcaption>{q.source}</figcaption>
              </figure>
            ))}
          </section>
        )}
        {!!kit?.releases.length && (
          <section aria-labelledby="pressReleaseList">
            <h2 id="pressReleaseList" className="aq-smartlink-list-title">보도자료 <span>{kit.releases.length}</span></h2>
            <ul className="aq-pr-list">{kit.releases.map(r => <PressReleaseCard key={r.id} r={r} />)}</ul>
          </section>
        )}
        {page.photo && !MOCK && (
          <section aria-labelledby="pressPhoto">
            <h2 id="pressPhoto" className="aq-smartlink-list-title">사진</h2>
            <div className="aq-press-photo">
              <img src={`${API_BASE}/api/artist-links/${encodeURIComponent(slug)}/photo?media=2${page.photo_v ? `&v=${page.photo_v}` : ''}`} alt={`${page.name} 프로필 사진`} loading="lazy" />
              <a className="ui-btn is-sm" href={`${API_BASE}/api/artist-links/${encodeURIComponent(slug)}/photo?media=2${page.photo_v ? `&v=${page.photo_v}` : ''}`}
                download={`${page.name}-profile.jpg`} target="_blank" rel="noopener noreferrer"><Glyph name="download" size={14} />프로필 사진 받기</a>
            </div>
          </section>
        )}
        <section aria-labelledby="pressReleases">
          <h2 id="pressReleases" className="aq-smartlink-list-title">발매 <span>{page.releases.length}</span></h2>
          {page.releases.length ? (
            <ul className="aq-press-releases">{page.releases.map(r => <PressRelease key={r.slug} r={r} artist={page.name} />)}</ul>
          ) : <p className="muted small">아직 공개된 발매가 없어요.</p>}
        </section>
        {(page.pages.length > 0 || page.links.length > 0) && (
          <section aria-labelledby="pressLinks">
            <h2 id="pressLinks" className="aq-smartlink-list-title">플랫폼·링크</h2>
            <ul className="ui-list is-inset aq-press-links">
              {page.pages.map(p => {
                const name = PLATFORM_NAMES[p.platform] ?? p.platform;
                return (
                  <li key={`p-${p.platform}`}>
                    <a className="ui-row is-flat" href={p.url} target="_blank" rel="noopener noreferrer nofollow">
                      <span className="aq-press-link"><BrandMark id={p.platform} label={name} size={28} /><span>{name}</span></span>
                      <Glyph name="arrow-up-right" size={15} className="aq-set-chev" />
                    </a>
                  </li>
                );
              })}
              {page.links.map((l, i) => {
                const brand = linkBrand(l.url);
                return (
                  <li key={`l-${i}`}>
                    <a className="ui-row is-flat" href={l.url} target="_blank" rel="noopener noreferrer nofollow">
                      <span className="aq-press-link">
                        {brand ? <BrandMark id={brand} label={l.label} size={28} /> : <span className="aq-artist-link-icon is-sm" aria-hidden="true"><Glyph name="link" size={15} /></span>}
                        <span>{l.label}<small>{shortUrl(l.url)}</small></span>
                      </span>
                      <Glyph name="arrow-up-right" size={15} className="aq-set-chev" />
                    </a>
                  </li>
                );
              })}
            </ul>
          </section>
        )}
        <PublicFoot here="press" artist={{ slug, name: page.name, news: (page.community?.posts_total ?? 0) > 0, releases: page.releases.length > 0 }} />
    </PublicShell>
  );
}
