// 아티스트 소식 전체 (/link/artist/:slug/news) — 아티스트가 스튜디오에서 올린 공지·이벤트. 고정한 글, 다가오는 이벤트(가까운 순),
// 최신 글 순서. 공지·이벤트로 걸러 볼 수 있다. 소개 페이지를 끄면 같이 닫힌다.
import { useEffect, useRef, useState } from 'react';
import { Link, useParams } from '../lib/router';
import { req } from '../api/http';
import { MOCK } from '../lib/mode';
import { gradientFor } from '../lib/catalog';
import { artistPath } from '../lib/publicLinks';
import { MOCK_POSTS, dday, type ArtistPost, type PostKind } from '../lib/artistCommunity';
import { PostCard } from '../components/ArtistCommunity';
import { Glyph } from '../components/Glyph';
import { PUBLIC_UNITS, useArtistPage } from './ArtistPage';
import { usePublicReveal } from '../hooks/usePublicReveal';

/** 소식·디스코그래피 카드가 불러오는 동안 — 카드 모양 그대로 반짝이는 자리 */
export function CardSkeleton({ rows = 3, tall = false }: { rows?: number; tall?: boolean }) {
  return (
    <div className={`aq-pub-skel${tall ? ' is-tall' : ''}`} role="status" aria-label="불러오는 중">
      {Array.from({ length: rows }, (_, i) => <span key={i} className="aq-skel" style={{ animationDelay: `${i * 70}ms` }} />)}
    </div>
  );
}

export function ArtistSubHead({ slug, name, title }: { slug: string; name: string; title: string }) {
  return (
    <header className="aq-artist-subhead">
      <Link to={artistPath(slug)} className="aq-artist-back" aria-label={`${name} 소개 페이지로`}>
        <Glyph name="chevron-left" size={18} />
        <span className="aq-artist-avatar is-sm" style={{ background: gradientFor(slug) }} aria-hidden="true">{name.trim().charAt(0)}</span>
        <span className="min-0"><strong>{name}</strong><small>{title}</small></span>
      </Link>
    </header>
  );
}

export function PageMissing() {
  return (
    <main className="aq-smartlink is-missing">
      <div className="aq-smartlink-card"><h1>페이지를 찾을 수 없어요</h1><p className="muted">페이지가 꺼졌거나 주소가 잘못됐어요.</p></div>
    </main>
  );
}

const FILTERS: { id: 'ALL' | PostKind; label: string }[] = [{ id: 'ALL', label: '전체' }, { id: 'NOTICE', label: '공지' }, { id: 'EVENT', label: '이벤트' }];

export function ArtistNews() {
  const { slug = '' } = useParams<{ slug: string }>();
  const { page, missing } = useArtistPage(slug, '소식 · AUDENIQ');
  const [posts, setPosts] = useState<ArtistPost[] | null>(null);
  const [filter, setFilter] = useState<'ALL' | PostKind>('ALL');
  // 거르기를 바꾸면 목록을 새로 그리며 줄이 차례로 떠오른다 (처음 열 때는 카드 등장에 맡긴다)
  const [changed, setChanged] = useState(false);
  const cardRef = useRef<HTMLDivElement>(null);
  usePublicReveal(cardRef, PUBLIC_UNITS, !!page && !!posts);
  useEffect(() => {
    if (MOCK) { const t = window.setTimeout(() => setPosts(MOCK_POSTS), 300); return () => window.clearTimeout(t); }
    req<{ items: ArtistPost[] }>(`/api/artist-links/${encodeURIComponent(slug)}/posts`, { quiet401: true })
      .then(r => setPosts(r.items), () => setPosts([]));
  }, [slug]);

  if (missing) return <PageMissing />;
  if (!page) return <main className="aq-smartlink" aria-busy="true" />;
  const all = posts ?? [];
  const shown = all.filter(p => filter === 'ALL' || p.kind === filter);
  const upcoming = all.filter(p => p.kind === 'EVENT' && p.starts_at && !dday(p.starts_at).past).length;

  return (
    <main className="aq-smartlink aq-artistpage">
      <div ref={cardRef} className="aq-smartlink-card aq-artist-card aq-artist-sub">
        <ArtistSubHead slug={slug} name={page.name} title={upcoming ? `소식 · 다가오는 이벤트 ${upcoming}개` : '소식'} />
        {posts === null && <CardSkeleton />}
        {all.length > 0 && (
          <div className="ui-chips aq-news-filter" role="group" aria-label="소식 종류">
            {FILTERS.map(f => (
              <button key={f.id} type="button" className="ui-chip is-sm" aria-pressed={filter === f.id} onClick={() => { setFilter(f.id); setChanged(true); }}>
                {f.label}<span className="ui-chip-count">{f.id === 'ALL' ? all.length : all.filter(p => p.kind === f.id).length}</span>
              </button>
            ))}
          </div>
        )}
        {posts && (shown.length > 0
          ? <ul key={filter} className={`aq-post-list${changed ? ' aq-stagger' : ''}`}>{shown.map(p => <li key={p.id}><PostCard post={p} artist={page.name} /></li>)}</ul>
          : <p key={filter} className={`aq-cheer-empty${changed ? ' aq-soft-in' : ''}`}><Glyph name="megaphone" size={18} />{all.length ? '이 종류의 소식은 아직 없어요.' : '아직 올라온 소식이 없어요.'}</p>)}
        <p className="aq-smartlink-foot"><Link to={artistPath(slug)}>{page.name}</Link> · Distributed by AUDENIQ</p>
      </div>
    </main>
  );
}
