// 아티스트 소개 페이지 (/link/artist/:slug) — 로그인 없이 누구나. 스마트링크처럼 SNS·프로필에 거는 한 주소:
// 이름·소개 글, 플랫폼별 아티스트 페이지, 스마트링크가 있는 발매(최신순). 발매를 누르면 그 스마트링크(/l/코드)로 간다.
// 아티스트가 직접 넣은 링크(인스타·유튜브·공연 예매…)도 줄로 — 프로필 링크 하나로 끝나게.
// 아티스트가 올린 소식(공지·이벤트, 고정·다가오는 이벤트 먼저 세 개)과 팬 응원도 여기에 — 전체 소식은 ArtistNews.tsx,
// 수록곡까지 보는 디스코그래피는 ArtistDiscography.tsx.
// 아티스트가 계정 정보에서 켜 둔 동안만 보인다. 조회 수는 아티스트에게 보인다. 보도자료 페이지는 ArtistPress.tsx.
import { useEffect, useState } from 'react';
import { Link, useParams } from '../lib/router';
import { API_BASE, req } from '../api/http';
import { MOCK } from '../lib/mode';
import { PLATFORM_NAMES } from '../lib/platformNames';
import { BrandMark } from '../components/BrandMark';
import { Glyph } from '../components/Glyph';
import { gradientFor } from '../lib/catalog';
import { linkBrand, shortUrl, type OwnLink } from '../lib/ownLinks';
import { artistDiscographyPath, artistNewsPath, artistPath, releasePath } from '../lib/publicLinks';
import { MOCK_CHEERS, MOCK_POSTS, type Community } from '../lib/artistCommunity';
import { CheerBoard, PostCard } from '../components/ArtistCommunity';
import { PublicFoot, PublicLoading, PublicMissing, PublicShell } from '../components/PublicPage';
import { CountUp } from '../ui';

/** 바탕에 흐리게 깔 커버 — 가장 최근 발매의 커버 (소개·소식·디스코그래피·보도자료가 같은 바탕) */
export function artistBackdrop(page: { releases: { slug: string; cover: boolean }[] } | null): string | undefined {
  const latest = page?.releases.find(r => r.cover);
  return latest && !MOCK ? `${API_BASE}/api/smart-links/${encodeURIComponent(latest.slug)}/cover` : undefined;
}

export interface ArtistRelease { slug: string; title: string; type: string; release_date: string | null; released: boolean; cover: boolean }
type Release = ArtistRelease;
export interface ArtistPageData {
  name: string; bio: string; links: OwnLink[]; contact: string;
  pages: { platform: string; url: string }[]; releases: Release[];
  community?: Community;
}
type Page = ArtistPageData;

export const RELEASE_TYPE: Record<string, string> = { SINGLE: '싱글', EP: 'EP', ALBUM: '정규' };
const TYPE = RELEASE_TYPE;

const MOCK_PAGE: Page = {
  name: '한결',
  bio: '도시의 풍경과 하루의 감정을 음악으로 기록해요.\n새벽에 듣기 좋은 노래를 만듭니다.',
  links: [{ label: '인스타그램', url: 'https://www.instagram.com/hangyeol' }, { label: '유튜브', url: 'https://www.youtube.com/@hangyeol' }, { label: '단독 공연 예매', url: 'https://tickets.example.com/hangyeol' }],
  contact: 'booking@hangyeol.example',
  pages: [{ platform: 'melon', url: 'https://www.melon.com/' }, { platform: 'spotify', url: 'https://open.spotify.com/' }, { platform: 'apple', url: 'https://music.apple.com/' }],
  releases: [
    { slug: 'soon0001', title: '겨울 산책', type: 'SINGLE', release_date: '2099-01-01', released: false, cover: false },
    { slug: 'mock0001', title: '여름의 끝', type: 'EP', release_date: '2026-08-01', released: true, cover: false },
    { slug: 'mock0002', title: '첫 번째 싱글', type: 'SINGLE', release_date: '2026-03-14', released: true, cover: false },
  ],
  community: { posts: MOCK_POSTS, posts_total: MOCK_POSTS.length, cheers_enabled: true, cheers_total: MOCK_CHEERS.length },
};

/** 'EP · 2026' · 발매 예정이면 '11월 15일 발매 예정' (올해가 아니면 연도도) */
function when(r: Release, year = new Date().getFullYear()): string {
  if (!r.release_date) return TYPE[r.type] ?? r.type;
  if (!r.released) {
    const y = Number(r.release_date.slice(0, 4));
    return `${y !== year ? `${y}년 ` : ''}${Number(r.release_date.slice(5, 7))}월 ${Number(r.release_date.slice(8, 10))}일 발매 예정`;
  }
  return `${TYPE[r.type] ?? r.type} · ${r.release_date.slice(0, 4)}`;
}

function ReleaseCard({ r, from }: { r: Release; from: { path: string; name: string } }) {
  const [ok, setOk] = useState(true);
  const cover = r.cover && ok && !MOCK ? `${API_BASE}/api/smart-links/${encodeURIComponent(r.slug)}/cover` : '';
  return (
    <li>
      <Link to={releasePath(r.slug)} state={{ artist: from }} className="aq-artist-release">
        {cover
          ? <img className="aq-artist-release-cover" src={cover} alt="" loading="lazy" onError={() => setOk(false)} />
          : <span className="aq-artist-release-cover is-empty" style={{ background: gradientFor(r.slug) }} aria-hidden="true"><Glyph name="music" className="aq-cover-glyph" /></span>}
        <span className="aq-artist-release-title">{r.title}</span>
        <span className={`aq-artist-release-meta${r.released ? '' : ' is-soon'}`}>{when(r)}</span>
      </Link>
    </li>
  );
}

export { when as releaseWhen };

/** 공개 아티스트 페이지 데이터 — 소개 페이지와 보도자료 페이지가 같이 쓴다. 없으면 missing */
export function useArtistPage(slug: string, titleSuffix = 'AUDENIQ') {
  const [page, setPage] = useState<Page | null>(null);
  const [missing, setMissing] = useState(false);
  useEffect(() => {
    if (MOCK) { setPage(MOCK_PAGE); return; }
    req<Page>(`/api/artist-links/${encodeURIComponent(slug)}`, { quiet401: true })
      .then(p => setPage({ ...p, links: p.links ?? [], contact: p.contact ?? '' }), () => setMissing(true));
  }, [slug]);
  useEffect(() => {
    if (page) document.title = `${page.name} · ${titleSuffix}`;
  }, [page, titleSuffix]);
  return { page, missing };
}

export function ArtistPage() {
  const { slug = '' } = useParams<{ slug: string }>();
  const { page, missing } = useArtistPage(slug);

  if (missing) return <PublicMissing />;
  if (!page) return <PublicLoading />;
  const news = page.community;

  return (
    <PublicShell bg={artistBackdrop(page)} className="aq-artistpage" cardClass="aq-artist-card">
        <span className="aq-artist-avatar" style={{ background: gradientFor(slug) }} aria-hidden="true">{page.name.trim().charAt(0)}</span>
        <h1>{page.name}</h1>
        {page.bio && <p className="aq-artist-bio">{page.bio}</p>}
        {page.pages.length > 0 && (
          <ul className="aq-artist-platforms" aria-label="플랫폼 아티스트 페이지">
            {page.pages.map(p => {
              const name = PLATFORM_NAMES[p.platform] ?? p.platform;
              return (
                <li key={p.platform}>
                  <a href={p.url} target="_blank" rel="noopener noreferrer nofollow" aria-label={`${name}에서 ${page.name} 보기`} title={name}>
                    <BrandMark id={p.platform} label={name} size={40} className="aq-smartlink-logo" />
                  </a>
                </li>
              );
            })}
          </ul>
        )}
        {page.links.length > 0 && (
          <ul className="aq-smartlink-list aq-artist-links" aria-label="링크">
            {page.links.map((l, i) => {
              const brand = linkBrand(l.url);
              return (
                <li key={`${i}-${l.url}`}>
                  <a href={l.url} target="_blank" rel="noopener noreferrer nofollow">
                    {brand ? <BrandMark id={brand} label={l.label} size={40} className="aq-smartlink-logo" />
                      : <span className="aq-artist-link-icon" aria-hidden="true"><Glyph name="link" size={18} /></span>}
                    <span className="aq-smartlink-name"><strong>{l.label}</strong><small>{shortUrl(l.url)}</small></span>
                    <Glyph name="chevron-right" size={16} className="aq-set-chev" />
                  </a>
                </li>
              );
            })}
          </ul>
        )}
        {news && news.posts_total > 0 && (
          <section className="aq-artist-news" aria-labelledby="artistNewsTitle">
            <div className="aq-artist-section-head">
              <h2 id="artistNewsTitle" className="aq-smartlink-list-title">소식 <span><CountUp value={news.posts_total} /></span></h2>
              {news.posts_total > news.posts.length && <Link to={artistNewsPath(slug)} className="aq-artist-more">전체 보기<Glyph name="chevron-right" size={14} /></Link>}
            </div>
            <ul className="aq-post-list">
              {news.posts.map(p => <li key={p.id}><PostCard post={p} artist={page.name} clamp /></li>)}
            </ul>
          </section>
        )}
        <section className="aq-artist-releases" aria-labelledby="artistReleasesTitle">
          <div className="aq-artist-section-head">
            <h2 id="artistReleasesTitle" className="aq-smartlink-list-title">발매 <span><CountUp value={page.releases.length} /></span></h2>
            {page.releases.length > 0 && <Link to={artistDiscographyPath(slug)} className="aq-artist-more">디스코그래피<Glyph name="chevron-right" size={14} /></Link>}
          </div>
          {page.releases.length > 0 ? (
            <ul className={`aq-artist-release-grid${page.releases.length === 1 ? ' is-single' : ''}`}>
              {page.releases.map(r => <ReleaseCard key={r.slug} r={r} from={{ path: artistPath(slug), name: page.name }} />)}
            </ul>
          ) : <p className="muted small">아직 공개된 발매가 없어요.</p>}
        </section>
        {news && <CheerBoard slug={slug} artist={page.name} total={news.cheers_total} enabled={news.cheers_enabled} />}
        <PublicFoot here="home" artist={{ slug, name: page.name, news: (news?.posts_total ?? 0) > 0, releases: page.releases.length > 0 }} />
    </PublicShell>
  );
}
