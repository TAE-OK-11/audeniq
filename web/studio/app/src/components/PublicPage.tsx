// 공개 화면 공통 바탕 — 스마트링크(발매), 아티스트 소개·소식·디스코그래피·보도자료가 같은 틀을 쓴다.
// 흐린 커버 바탕, 같은 폭·여백·모서리의 카드, 같은 불러오는 모양·없는 페이지 안내, 하위 화면 머리(돌아가기·아바타·이름),
// 맨 아래 형제 화면 링크(소개·소식·디스코그래피·보도자료)와 ‘Distributed by AUDENIQ’. 화면 아래 덩어리는 스크롤로 떠오른다.
import { useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { Link } from '../lib/router';
import { gradientFor } from '../lib/catalog';
import { artistDiscographyPath, artistNewsPath, artistPath, artistPressPath } from '../lib/publicLinks';
import { usePublicReveal } from '../hooks/usePublicReveal';
import { Glyph } from './Glyph';
import { API_BASE } from '../api/http';
import { MOCK } from '../lib/mode';

/** 스크롤로 떠오르는 덩어리 — 소식 카드, 발매 칸, 응원 폼·줄, 디스코그래피, 플랫폼 줄, 가사 */
export const PUBLIC_UNITS = [
  '.aq-artist-section-head', '.aq-post-list>li', '.aq-artist-release-grid>li', '.aq-cheers>h2', '.aq-cheer-form',
  '.aq-cheer-list:not(.is-skeleton)>li', '.aq-disco-item', '.aq-disco-year>h2', '.aq-press-release', '.aq-smartlink-list>li',
  '.aq-smartlink-lyrics .aq-faq-item',
].join(', ');

export function PublicShell({ bg, className, cardClass, ready = true, children }: {
  /** 흐리게 깔 커버 주소 — 없으면 옅은 회색 바탕 */
  bg?: string; className?: string; cardClass?: string; ready?: boolean; children: ReactNode;
}) {
  const card = useRef<HTMLDivElement>(null);
  usePublicReveal(card, PUBLIC_UNITS, ready);
  return (
    <main className={`aq-smartlink aq-pub${className ? ` ${className}` : ''}`}
      style={bg ? ({ '--aq-pub-bg': `url("${bg}")` } as CSSProperties) : undefined}>
      {bg && <div className="aq-smartlink-bg" style={{ backgroundImage: `url("${bg}")` }} aria-hidden="true" />}
      <div ref={card} className={`aq-smartlink-card aq-pub-card${cardClass ? ` ${cardClass}` : ''}`}>{children}</div>
    </main>
  );
}

/** 불러오는 동안 — 카드 틀 그대로 머리·줄 자리가 반짝인다 (빈 회색 화면 대신) */
export function PublicLoading({ hero = 'avatar' }: { hero?: 'avatar' | 'cover' }) {
  return (
    <main className="aq-smartlink aq-pub" aria-busy="true">
      <div className="aq-smartlink-card aq-pub-card aq-pub-loading" role="status" aria-label="불러오는 중">
        <span className={`aq-skel aq-pub-loading-hero is-${hero}`} />
        <span className="aq-skel aq-pub-loading-title" />
        <span className="aq-skel aq-pub-loading-sub" />
        {[0, 1, 2].map(i => <span key={i} className="aq-skel aq-pub-loading-row" style={{ animationDelay: `${i * 70}ms` }} />)}
      </div>
    </main>
  );
}

export function PublicMissing({ text = '페이지가 꺼졌거나 주소가 잘못됐어요.' }: { text?: string }) {
  return (
    <main className="aq-smartlink aq-pub is-missing">
      <div className="aq-smartlink-card aq-pub-card aq-pub-missing">
        <span className="aq-pub-missing-icon" aria-hidden="true"><Glyph name="link" size={22} /></span>
        <h1>페이지를 찾을 수 없어요</h1>
        <p className="muted">{text}</p>
      </div>
    </main>
  );
}

/** 아티스트 사진 — 스튜디오에서 올린 프로필 사진, 없거나 못 불러오면 이름 첫 글자 */
export function ArtistAvatar({ slug, name, photo, version, sm }: { slug: string; name: string; photo?: boolean; version?: number | null; sm?: boolean }) {
  const [failed, setFailed] = useState(false);
  const cls = `aq-artist-avatar${sm ? ' is-sm' : ''}`;
  if (photo && !failed && !MOCK) {
    return (
      <span className={`${cls} has-photo`} aria-hidden="true">
        <img src={`${API_BASE}/api/artist-links/${encodeURIComponent(slug)}/photo${version ? `?v=${version}` : ''}`} alt="" decoding="async" onError={() => setFailed(true)} />
      </span>
    );
  }
  return <span className={cls} style={{ background: gradientFor(slug) }} aria-hidden="true">{name.trim().charAt(0)}</span>;
}

/** 하위 화면 머리 — 돌아가기 · 아바타 · 이름 · 화면 이름 (소식·디스코그래피·보도자료, 아티스트에서 들어온 발매) */
export function ArtistSubHead({ slug, name, title, to, photo, version }: { slug: string; name: string; title: string; to?: string; photo?: boolean; version?: number | null }) {
  return (
    <header className="aq-artist-subhead">
      <Link to={to ?? artistPath(slug)} className="aq-artist-back" aria-label={`${name} 소개 페이지로`}>
        <Glyph name="chevron-left" size={18} />
        <ArtistAvatar slug={slug} name={name} photo={photo} version={version} sm />
        <span className="min-0"><strong>{name}</strong><small>{title}</small></span>
      </Link>
    </header>
  );
}

type Here = 'home' | 'news' | 'disco' | 'press';
/** 맨 아래 — 같은 아티스트의 다른 화면으로 (지금 화면은 빼고) + Distributed by AUDENIQ */
export function PublicFoot({ artist, here }: { artist?: { slug: string; name: string; news?: boolean; releases?: boolean }; here?: Here }) {
  const links: [Here, string, string][] = artist ? ([
    ['home', artist.name, artistPath(artist.slug)],
    ...(artist.news ? [['news', '소식', artistNewsPath(artist.slug)]] as [Here, string, string][] : []),
    ...(artist.releases ? [['disco', '디스코그래피', artistDiscographyPath(artist.slug)]] as [Here, string, string][] : []),
    ['press', '보도자료', artistPressPath(artist.slug)],
  ] as [Here, string, string][]).filter(([k]) => k !== here) : [];
  return (
    <footer className="aq-pub-foot">
      {links.length > 0 && (
        <nav aria-label={`${artist?.name} 다른 화면`}>
          {links.map(([k, label, to]) => <Link key={k} to={to}>{label}</Link>)}
        </nav>
      )}
      <a className="aq-pub-brand" href="https://audeniq.com" target="_blank" rel="noopener noreferrer" aria-label="Distributed by AUDENIQ">
        <small>Distributed by</small>
        <img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="" />
      </a>
    </footer>
  );
}
