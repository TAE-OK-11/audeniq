// 보도자료 페이지 (/a/:slug/press) — 아티스트 소개 페이지의 변형. 플레이리스트 큐레이터·기자·공연 기획자에게 보내는 한 장:
// 소개, 연락처(아티스트가 넣었을 때만), 발매마다 스마트링크·커버 받기·홍보 이미지, 플랫폼 페이지와 링크.
// 소개 페이지와 같은 데이터(useArtistPage)라 소개 페이지를 끄면 이 페이지도 닫힌다.
import { useState } from 'react';
import { Link, useParams } from '../lib/router';
import { API_BASE } from '../api/http';
import { MOCK } from '../lib/mode';
import { PLATFORM_NAMES } from '../lib/platformNames';
import { gradientFor } from '../lib/catalog';
import { linkBrand, shortUrl } from '../lib/ownLinks';
import { BrandMark } from '../components/BrandMark';
import { Glyph } from '../components/Glyph';
import { PromoImageButton } from '../components/PromoImageButton';
import { RELEASE_TYPE, useArtistPage, type ArtistRelease } from './ArtistPage';

const coverUrl = (slug: string) => `${API_BASE}/api/smart-links/${encodeURIComponent(slug)}/cover`;
const dotted = (d: string | null) => (d ? d.replace(/-/g, '.') : '발매일 미정');

function PressRelease({ r, artist }: { r: ArtistRelease; artist: string }) {
  const [ok, setOk] = useState(true);
  const cover = r.cover && ok && !MOCK ? coverUrl(r.slug) : '';
  const link = `${window.location.origin}/l/${r.slug}`;
  return (
    <li className="aq-press-release">
      {cover
        ? <img className="aq-press-cover" src={cover} alt="" loading="lazy" onError={() => setOk(false)} />
        : <span className="aq-press-cover is-empty" style={{ background: gradientFor(r.slug) }} aria-hidden="true"><Glyph name="music" className="aq-cover-glyph" /></span>}
      <div className="min-0">
        <strong className="ui-row-title">{r.title}</strong>
        <span className="ui-row-meta"><span>{RELEASE_TYPE[r.type] ?? r.type}</span><span>{dotted(r.release_date)}{r.released ? '' : ' 발매 예정'}</span></span>
        <div className="aq-press-actions">
          <Link to={`/l/${encodeURIComponent(r.slug)}`} className="ui-btn is-sm"><Glyph name="link" size={14} />스마트링크</Link>
          {cover && <a className="ui-btn is-sm" href={cover} download={`${r.title}-cover.jpg`} target="_blank" rel="noopener noreferrer"><Glyph name="download" size={14} />커버 받기</a>}
          <PromoImageButton input={{ title: r.title, artist, releaseDate: r.release_date, url: link, cover: cover || undefined }} filename={`audeniq-${r.slug}`} />
        </div>
      </div>
    </li>
  );
}

export function ArtistPress() {
  const { slug = '' } = useParams<{ slug: string }>();
  const { page, missing } = useArtistPage(slug, '보도자료 · AUDENIQ');

  if (missing) {
    return (
      <main className="aq-smartlink is-missing">
        <div className="aq-smartlink-card"><h1>페이지를 찾을 수 없어요</h1><p className="muted">페이지가 꺼졌거나 주소가 잘못됐어요.</p></div>
      </main>
    );
  }
  if (!page) return <main className="aq-smartlink" aria-busy="true" />;
  const latest = page.releases[0];

  return (
    <main className="aq-smartlink aq-artistpage aq-presspage">
      <article className="aq-smartlink-card aq-artist-card aq-press-card">
        <p className="aq-press-eyebrow">PRESS KIT · 보도자료</p>
        <header className="aq-press-head">
          <span className="aq-artist-avatar" style={{ background: gradientFor(slug) }} aria-hidden="true">{page.name.trim().charAt(0)}</span>
          <div className="min-0">
            <h1>{page.name}</h1>
            <p className="ui-row-meta">
              <span>발매 {page.releases.length}개</span>
              {latest && <span>최근 ‘{latest.title}’ {dotted(latest.release_date)}</span>}
            </p>
          </div>
        </header>
        {page.contact && (
          <div className="aq-press-contact">
            <a className="ui-btn is-primary is-block" href={`mailto:${page.contact}?subject=${encodeURIComponent(`[${page.name}] 문의`)}`}>
              <Glyph name="mail" size={16} className="aq-glyph-lead" />섭외·문의 메일 보내기
            </a>
            <small>{page.contact}</small>
          </div>
        )}
        {page.bio && (
          <section aria-labelledby="pressBio">
            <h2 id="pressBio" className="aq-smartlink-list-title">소개</h2>
            <p className="aq-artist-bio aq-press-bio">{page.bio}</p>
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
        <p className="aq-smartlink-foot"><Link to={`/a/${slug}`}>아티스트 페이지</Link> · Distributed by AUDENIQ</p>
      </article>
    </main>
  );
}
