// 아티스트 디스코그래피 (/link/artist/:slug/discography) — 소개 페이지에 보이는 발매(스마트링크를 켠 것)를 연도별로, 수록곡까지.
// 발매를 누르면 그 스마트링크로. 다른 아티스트의 곡(피처링·컴필레이션)은 곡 옆에 아티스트 이름을 붙인다.
import { useEffect, useState } from 'react';
import { Link, useParams } from '../lib/router';
import { API_BASE, req } from '../api/http';
import { MOCK } from '../lib/mode';
import { gradientFor } from '../lib/catalog';
import { releasePath } from '../lib/publicLinks';
import type { DiscoTrack } from '../lib/artistCommunity';
import { Glyph } from '../components/Glyph';
import { Tag } from '../ui';
import { ArtistSubHead, PublicFoot, PublicLoading, PublicMissing, PublicShell } from '../components/PublicPage';
import { RELEASE_TYPE, artistBackdrop, releaseWhen, useArtistPage, type ArtistRelease } from './ArtistPage';
import { CardSkeleton } from './ArtistNews';

export interface DiscoRelease extends ArtistRelease { tracks: DiscoTrack[] }

const MOCK_DISCO: DiscoRelease[] = [
  { slug: 'soon0001', title: '겨울 산책', type: 'SINGLE', release_date: '2099-01-01', released: false, cover: false, tracks: [{ title: '겨울 산책', disc: 1, number: 1, artist: null }] },
  { slug: 'mock0001', title: '여름의 끝', type: 'EP', release_date: '2026-08-01', released: true, cover: false, tracks: [
    { title: '여름의 끝', disc: 1, number: 1, artist: null }, { title: '밤의 도시', disc: 1, number: 2, artist: null },
    { title: '파도 (Feat. 서린)', disc: 1, number: 3, artist: null }, { title: '안녕, 여름', disc: 1, number: 4, artist: null },
    { title: '늦은 오후', disc: 1, number: 5, artist: null }, { title: '바다로 가는 길', disc: 1, number: 6, artist: null },
    { title: '여름의 끝 (Acoustic)', disc: 1, number: 7, artist: null }, { title: '여름의 끝 (Inst.)', disc: 1, number: 8, artist: null },
  ] },
  { slug: 'mock0002', title: '첫 번째 싱글', type: 'SINGLE', release_date: '2026-03-14', released: true, cover: false, tracks: [{ title: '첫 번째 싱글', disc: 1, number: 1, artist: null }, { title: '첫 번째 싱글 (Inst.)', disc: 1, number: 2, artist: null }] },
];

/** 연도별 묶음 — 발매 예정이 맨 위, 날짜 없는 발매는 맨 아래 */
export function groupByYear(items: DiscoRelease[]): { label: string; items: DiscoRelease[] }[] {
  const groups: { label: string; items: DiscoRelease[] }[] = [];
  for (const r of items) {
    const label = !r.release_date ? '발매일 미정' : !r.released ? '발매 예정' : `${r.release_date.slice(0, 4)}년`;
    const g = groups.find(x => x.label === label);
    if (g) g.items.push(r); else groups.push({ label, items: [r] });
  }
  const rank = (l: string) => (l === '발매 예정' ? 0 : l === '발매일 미정' ? 2 : 1);
  return groups.sort((a, b) => rank(a.label) - rank(b.label));
}

function DiscoItem({ r }: { r: DiscoRelease }) {
  const [ok, setOk] = useState(true);
  const [open, setOpen] = useState(false);
  const fold = r.tracks.length > 6;
  const cover = r.cover && ok && !MOCK ? `${API_BASE}/api/smart-links/${encodeURIComponent(r.slug)}/cover` : '';
  const discs = new Set(r.tracks.map(t => t.disc)).size;
  const row = (t: DiscoTrack) => (
    <li key={`${t.disc}-${t.number}`}>
      <span className="aq-disco-no">{discs > 1 ? `${t.disc}-${t.number}` : t.number}</span>
      <span className="aq-disco-title">{t.title}{t.artist && <small> · {t.artist}</small>}</span>
    </li>
  );
  return (
    <li className="aq-disco-item">
      <Link to={releasePath(r.slug)} className="aq-disco-head">
        {cover
          ? <img className="aq-disco-cover" src={cover} alt="" loading="lazy" onError={() => setOk(false)} />
          : <span className="aq-disco-cover is-empty" style={{ background: gradientFor(r.slug) }} aria-hidden="true"><Glyph name="music" className="aq-cover-glyph" /></span>}
        <span className="min-0">
          <strong>{r.title}</strong>
          <small className={r.released ? undefined : 'is-soon'}>{r.released ? `${RELEASE_TYPE[r.type] ?? r.type} · ${r.release_date?.replace(/-/g, '.') ?? ''}` : releaseWhen(r)}</small>
          <small>{r.tracks.length}곡</small>
        </span>
        <span className="aq-disco-go" aria-hidden="true"><Glyph name={r.released ? 'arrow-up-right' : 'bell'} size={16} /></span>
      </Link>
      {r.tracks.length > 0 && (
        <ol className="aq-disco-tracks" aria-label={`${r.title} 수록곡`}>
          {(fold ? r.tracks.slice(0, 5) : r.tracks).map(row)}
        </ol>
      )}
      {fold && (
        <>
          {/* 나머지 곡 — 높이가 0에서 부드럽게 열린다 (grid 0fr → 1fr) */}
          <div className={`aq-disco-rest${open ? ' is-open' : ''}`} inert={!open}>
            <ol className="aq-disco-tracks is-rest" start={6}>{r.tracks.slice(5).map(row)}</ol>
          </div>
          <button type="button" className={`aq-disco-all${open ? ' is-open' : ''}`} aria-expanded={open} onClick={() => setOpen(o => !o)}>
            {open ? '접기' : `수록곡 ${r.tracks.length}곡 모두 보기`}<Glyph name="chevron-down" size={14} />
          </button>
        </>
      )}
    </li>
  );
}

export function ArtistDiscography() {
  const { slug = '' } = useParams<{ slug: string }>();
  const { page, missing } = useArtistPage(slug, '디스코그래피 · AUDENIQ');
  const [items, setItems] = useState<DiscoRelease[] | null>(null);
  useEffect(() => {
    if (MOCK) { const t = window.setTimeout(() => setItems(MOCK_DISCO), 300); return () => window.clearTimeout(t); }
    req<{ items: DiscoRelease[] }>(`/api/artist-links/${encodeURIComponent(slug)}/discography`, { quiet401: true })
      .then(r => setItems(r.items), () => setItems([]));
  }, [slug]);

  if (missing) return <PublicMissing />;
  if (!page) return <PublicLoading />;
  const list = items ?? [];
  const count = (t: string) => list.filter(r => r.type === t).length;
  const summary = [['ALBUM', '정규'], ['EP', 'EP'], ['SINGLE', '싱글']]
    .filter(([t]) => count(t) > 0).map(([t, l]) => `${l} ${count(t)}`);
  const tracks = list.reduce((n, r) => n + r.tracks.length, 0);

  return (
    <PublicShell bg={artistBackdrop(page)} className="aq-artistpage" cardClass="aq-artist-card aq-artist-sub">
      <ArtistSubHead slug={slug} name={page.name} title="디스코그래피" />
      {items === null && <CardSkeleton rows={3} tall />}
      {list.length > 0 && (
        <p className="aq-disco-stats" aria-label="요약">
          {summary.map(x => <Tag key={x}>{x}</Tag>)}
          <Tag>수록곡 {tracks}</Tag>
        </p>
      )}
      {items && list.length === 0 && <p className="aq-cheer-empty"><Glyph name="music" size={18} />아직 공개된 발매가 없어요.</p>}
      {groupByYear(list).map(g => (
        <section key={g.label} className="aq-disco-year" aria-label={g.label}>
          <h2 className="aq-smartlink-list-title">{g.label}</h2>
          <ul className="aq-disco-list">{g.items.map(r => <DiscoItem key={r.slug} r={r} />)}</ul>
        </section>
      ))}
      <PublicFoot here="disco" artist={{ slug, name: page.name, news: (page.community?.posts_total ?? 0) > 0, releases: true }} />
    </PublicShell>
  );
}
