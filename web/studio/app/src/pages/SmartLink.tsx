// 스마트링크 (/l/:slug) — 로그인 없이 누구나. 발매 하나의 공유 페이지: 커버·제목·아티스트와 플랫폼별 듣기 링크.
// 아티스트가 발매 상세에서 링크를 넣고 켜 둔 동안만 보인다. 조회·클릭 수는 아티스트에게 보인다.
import { useEffect, useState } from 'react';
import { useParams } from '../lib/router';
import { API_BASE, req } from '../api/http';
import { errorMessage } from '../api/errors';
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
  const [email, setEmail] = useState('');
  const [agree, setAgree] = useState(false);
  const [alertState, setAlertState] = useState<'idle' | 'busy' | 'done'>('idle');
  const [alertError, setAlertError] = useState('');
  const base = `/api/smart-links/${encodeURIComponent(slug)}`;

  useEffect(() => {
    // 체험 모드: 'soon'으로 시작하는 코드는 발매 전 화면
    if (MOCK) { setPage(slug.startsWith('soon') ? { ...MOCK_PAGE, released: false, release_date: '2099-01-01' } : MOCK_PAGE); return; }
    req<Page>(base, { quiet401: true }).then(setPage, () => setMissing(true));
  }, [base, slug]);
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

  const signUp = async () => {
    setAlertError('');
    if (!email.trim()) { setAlertError('이메일을 적어 주세요.'); return; }
    if (!agree) { setAlertError('안내에 동의해 주세요.'); return; }
    if (MOCK) { setAlertState('done'); return; }
    setAlertState('busy');
    try {
      await req(`${base}/alerts`, { method: 'POST', body: { email: email.trim(), consent: true } });
      setAlertState('done');
    } catch (e) {
      setAlertError(errorMessage(e));
      setAlertState('idle');
    }
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
        {!page.released && page.release_date && (
          alertState === 'done' ? (
            <p className="aq-smartlink-alert-done" role="status">발매되면 메일로 한 번 알려 드릴게요.</p>
          ) : (
            <form className="aq-smartlink-alert" onSubmit={e => { e.preventDefault(); void signUp(); }}>
              <label htmlFor="slAlertEmail">발매되면 알림 받기</label>
              <div className="aq-smartlink-alert-row">
                <input id="slAlertEmail" className="ui-input" type="email" inputMode="email" autoComplete="email" maxLength={254}
                  placeholder="이메일 주소" value={email} onChange={e => setEmail(e.target.value)} />
                <button type="submit" className="ui-btn is-primary" disabled={alertState === 'busy'}>{alertState === 'busy' ? '신청 중' : '신청'}</button>
              </div>
              <label className="aq-smartlink-consent">
                <input type="checkbox" checked={agree} onChange={e => setAgree(e.target.checked)} />
                <span>발매 알림 메일 1회 발송을 위해 이메일을 받는 데 동의해요. 메일을 보내면 주소는 바로 지워요. (발매일 1주 뒤까지 못 보내면 그때 지워요)</span>
              </label>
              {alertError && <p className="aq-smartlink-alert-err" role="alert">{alertError}</p>}
            </form>
          )
        )}
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
