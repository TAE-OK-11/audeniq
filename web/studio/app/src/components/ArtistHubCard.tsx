// 계정 정보 > 아티스트 페이지 — 아티스트마다 두 가지를 한 묶음으로: 플랫폼(Spotify·멜론 등) 아티스트 페이지 등록과
// 팬에게 보여 줄 소개 페이지(/a/코드). 예전엔 '아티스트 페이지'와 '아티스트 소개 페이지' 두 카드에 같은 이름이
// 두 번씩 나와 무엇이 무엇인지 헷갈렸다. 줄을 누르면 각자의 편집 창이 열린다 (ArtistPagesCard·ArtistLinkCard의 편집 창).
// 소개 페이지가 있으면 ‘소식’(공지·이벤트 올리기)과 ‘팬 응원’(숨기기·하트·받기 끄기) 줄도 (ArtistCommunityManager).
import { useCallback, useEffect, useState } from 'react';
import { orgPath, req } from '../api/http';
import { MOCK } from '../lib/mode';
import { useProfile } from '../store/profile';
import { Modal } from './Modal';
import { Glyph } from './Glyph';
import { PagesEditor, pagesSummary, type ArtistRow } from './ArtistPagesCard';
import { LinkEditor, linkSummary, type ArtistLinkRow } from './ArtistLinkCard';
import { CheersManager, PostsManager } from './ArtistCommunityManager';
import { num } from '../lib/format';

export function ArtistHubCard() {
  const profile = useProfile();
  const [pages, setPages] = useState<ArtistRow[]>([]);
  const [links, setLinks] = useState<ArtistLinkRow[]>([]);
  const [editPages, setEditPages] = useState<ArtistRow | null>(null);
  const [editLink, setEditLink] = useState<ArtistLinkRow | null>(null);
  const [news, setNews] = useState<ArtistLinkRow | null>(null);
  const [cheers, setCheers] = useState<ArtistLinkRow | null>(null);
  const patchLink = useCallback((id: string, patch: Partial<NonNullable<ArtistLinkRow['link']>>) =>
    setLinks(list => list.map(x => (x.artist_id === id && x.link ? { ...x, link: { ...x.link, ...patch } } : x))), []);
  const setPostCount = useCallback((n: number) => { if (news) patchLink(news.artist_id, { posts: n }); }, [news, patchLink]);
  const setCheerCount = useCallback((n: number) => { if (cheers) patchLink(cheers.artist_id, { cheers: n }); }, [cheers, patchLink]);
  // 받기 켜고 끄기 — 소개 페이지 저장(PUT)은 빠진 칸을 비우므로 지금 값을 함께 보낸다
  const setCheersEnabled = async (v: boolean) => {
    const row = links.find(x => x.artist_id === cheers?.artist_id);
    const l = row?.link;
    if (!row || !l) return;
    if (!MOCK) {
      await req(orgPath(`/artists/${encodeURIComponent(row.artist_id)}/link`), {
        method: 'PUT', body: { enabled: l.enabled, bio: l.bio, links: l.links ?? [], contact: l.contact ?? '', cheers: v },
      });
    }
    patchLink(row.artist_id, { cheers_enabled: v });
  };

  const loadPages = useCallback(async () => {
    if (MOCK) return;
    try { setPages((await req<{ items: ArtistRow[] }>(orgPath('/artist-pages/all'), { quiet401: true })).items.filter(a => a.releases > 0)); }
    catch { /* 볼 수 없으면 그 줄만 뺀다 */ }
  }, []);
  const loadLinks = useCallback(async () => {
    // 체험 모드: 활동명으로 아티스트 하나 (저장은 이 화면 안에서만)
    if (MOCK) { setLinks(cur => (cur.length ? cur : [{ artist_id: 'mock', name: profile.name.trim() || '서린', releases: 2, link: null }])); return; }
    try { setLinks((await req<{ items: ArtistLinkRow[] }>(orgPath('/artist-links'), { quiet401: true })).items); }
    catch { /* 볼 수 없으면 그 줄만 뺀다 */ }
  }, [profile.name]);
  useEffect(() => { void loadPages(); void loadLinks(); }, [loadPages, loadLinks]);

  // 두 목록의 아티스트를 한 번씩, 소개 페이지 목록 순서(이름순)를 먼저
  const ids = [...new Set([...links.map(a => a.artist_id), ...pages.map(a => a.artist_id)])];
  if (!ids.length) return null;

  return (
    <section className="aq-set-card aq-artist-hub" aria-labelledby="setArtistHub">
      <h2 id="setArtistHub">아티스트 페이지</h2>
      <p className="ui-field-help aq-set-help">
        <b>플랫폼 페이지</b>는 Spotify·멜론 등에 아티스트를 정확히 연결하고, <b>소개 페이지</b>는 이름·소개·발매를 한 주소로 모아 SNS에 걸어요.
      </p>
      {ids.map(id => {
        const pg = pages.find(a => a.artist_id === id);
        const ln = links.find(a => a.artist_id === id);
        const name = ln?.name ?? pg?.name ?? '';
        const ps = pg && pagesSummary(pg);
        const ls = ln && linkSummary(ln);
        return (
          <div key={id} className="aq-artist-hub-group" role="group" aria-label={name}>
            <p className="aq-artist-hub-name">{name}</p>
            {pg && ps && (
              <button type="button" className="aq-set-row" onClick={() => setEditPages(pg)}>
                <span className="aq-set-label">플랫폼 페이지</span>
                <span className={`aq-set-value${ps.empty ? ' is-empty' : ''}`}>{ps.text}</span>
                <Glyph name="chevron-right" size={16} className="aq-set-chev" />
              </button>
            )}
            {ln && ls && (
              <button type="button" className="aq-set-row" onClick={() => setEditLink(ln)}>
                <span className="aq-set-label">소개 페이지</span>
                <span className={`aq-set-value${ls.empty ? ' is-empty' : ''}`}>{ls.text}</span>
                <Glyph name="chevron-right" size={16} className="aq-set-chev" />
              </button>
            )}
            {ln?.link && (
              <>
                <button type="button" className="aq-set-row" onClick={() => setNews(ln)}>
                  <span className="aq-set-label">소식</span>
                  <span className={`aq-set-value${ln.link.posts ? '' : ' is-empty'}`}>{ln.link.posts ? `${num(ln.link.posts)}개` : '공지·이벤트 올리기'}</span>
                  <Glyph name="chevron-right" size={16} className="aq-set-chev" />
                </button>
                <button type="button" className="aq-set-row" onClick={() => setCheers(ln)}>
                  <span className="aq-set-label">팬 응원</span>
                  <span className={`aq-set-value${ln.link.cheers ? '' : ' is-empty'}`}>
                    {ln.link.cheers_enabled === false ? '꺼짐' : ln.link.cheers ? `${num(ln.link.cheers)}개` : '아직 없어요'}
                  </span>
                  <Glyph name="chevron-right" size={16} className="aq-set-chev" />
                </button>
              </>
            )}
          </div>
        );
      })}
      {editPages && (
        <Modal title={`${editPages.name} 플랫폼 페이지`} onClose={() => setEditPages(null)} modalClass="aq-set-modal">
          <PagesEditor artist={editPages} onSaved={() => void loadPages()} />
        </Modal>
      )}
      {editLink && (
        <Modal title={`${editLink.name} 소개 페이지`} onClose={() => setEditLink(null)} modalClass="aq-set-modal">
          <LinkEditor artist={editLink} onSaved={row => setLinks(list => list.map(x => (x.artist_id === row.artist_id ? row : x)))} />
        </Modal>
      )}
      {news?.link && (
        <Modal title={`${news.name} 소식`} onClose={() => setNews(null)} modalClass="aq-set-modal">
          <PostsManager artistId={news.artist_id} artist={news.name} slug={news.link.slug} onCount={setPostCount} />
        </Modal>
      )}
      {cheers?.link && (
        <Modal title={`${cheers.name} 팬 응원`} onClose={() => setCheers(null)} modalClass="aq-set-modal">
          <CheersManager artistId={cheers.artist_id} artist={cheers.name}
            enabled={links.find(x => x.artist_id === cheers.artist_id)?.link?.cheers_enabled !== false}
            onEnabled={setCheersEnabled} onCount={setCheerCount} />
        </Modal>
      )}
    </section>
  );
}
