// 스마트링크 (/l/:slug) — 로그인 없이 누구나. 발매 하나의 공유 페이지: 커버·제목·아티스트와 플랫폼별 듣기 링크.
// 아티스트가 발매 상세에서 링크를 넣고 켜 둔 동안만 보인다. 조회·클릭 수는 아티스트에게 보인다.
import { useEffect, useState } from 'react';
import { useParams } from '../lib/router';
import { API_BASE, req } from '../api/http';
import { MOCK } from '../lib/mode';
import { PLATFORM_NAMES } from '../lib/platformNames';

interface Page {
  title: string; artist: string | null; release_date: string | null; released: boolean; cover: boolean;
  links: { platform: string; url: string }[];
}

const MOCK_PAGE: Page = {
  title: '여름의 끝', artist: '한결', release_date: '2026-08-01', released: true, cover: false,
  links: [{ platform: 'melon', url: 'https://www.melon.com/' }, { platform: 'spotify', url: 'https://open.spotify.com/' }],
};

export function SmartLink() {
  const { slug = '' } = useParams<{ slug: string }>();
  const [page, setPage] = useState<Page | null>(null);
  const [missing, setMissing] = useState(false);
  const [coverOk, setCoverOk] = useState(true);
  const base = `/api/smart-links/${encodeURIComponent(slug)}`;

  useEffect(() => {
    if (MOCK) { setPage(MOCK_PAGE); return; }
    req<Page>(base, { quiet401: true }).then(setPage, () => setMissing(true));
  }, [base]);
  useEffect(() => {
    if (page) document.title = `${page.title}${page.artist ? ` · ${page.artist}` : ''}`;
  }, [page]);

  const open = (platform: string) => {
    if (MOCK) return;
    // 페이지를 떠나도 클릭 기록은 보낸다
    void fetch(`${API_BASE}${base}/click`, {
      method: 'POST', keepalive: true, credentials: 'omit',
      headers: { 'Content-Type': 'application/json', Accept: 'application/json' }, body: JSON.stringify({ platform }),
    }).catch(() => undefined);
  };

  if (missing) {
    return (
      <main className="aq-smartlink is-missing">
        <div className="aq-smartlink-card"><h1>페이지를 찾을 수 없어요</h1><p className="muted">링크가 꺼졌거나 주소가 잘못됐어요.</p></div>
      </main>
    );
  }
  if (!page) return <main className="aq-smartlink" aria-busy="true" />;
  const cover = page.cover && coverOk ? `${API_BASE}${base}/cover` : '';

  return (
    <main className="aq-smartlink">
      {cover && <div className="aq-smartlink-bg" style={{ backgroundImage: `url("${cover}")` }} aria-hidden="true" />}
      <div className="aq-smartlink-card">
        {cover ? <img className="aq-smartlink-cover" src={cover} alt={`${page.title} 커버`} onError={() => setCoverOk(false)} />
          : <div className="aq-smartlink-cover is-empty" aria-hidden="true" />}
        <h1>{page.title}</h1>
        {page.artist && <p className="aq-smartlink-artist">{page.artist}</p>}
        {!page.released && page.release_date && <p className="aq-smartlink-soon">{page.release_date} 발매 예정</p>}
        {page.links.length > 0 ? (
          <ul className="aq-smartlink-list">
            {page.links.map(l => (
              <li key={l.platform}>
                <a href={l.url} rel="noopener noreferrer nofollow" onClick={() => open(l.platform)}>
                  <span>{PLATFORM_NAMES[l.platform] ?? l.platform}</span><b>{page.released ? '듣기' : '보기'}</b>
                </a>
              </li>
            ))}
          </ul>
        ) : <p className="muted small">아직 등록된 플랫폼 링크가 없어요.</p>}
        <p className="aq-smartlink-foot">Distributed by AUDENIQ</p>
      </div>
    </main>
  );
}
