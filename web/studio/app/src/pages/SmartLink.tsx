// 스마트링크 (/l/:slug) — 로그인 없이 누구나. 발매 하나의 공유 페이지: 커버·제목·아티스트와 플랫폼별 듣기 링크.
// 아티스트가 발매 상세에서 링크를 넣고 켜 둔 동안만 보인다. 조회·클릭 수는 아티스트에게 보인다.
import { useEffect, useState } from 'react';
import { countdownLabel, releaseMoment } from '../lib/countdown';
import { Link, useLocation, useParams } from '../lib/router';
import { API_BASE, req } from '../api/http';
import { errorMessage } from '../api/errors';
import { MOCK } from '../lib/mode';
import { PLATFORM_NAMES } from '../lib/platformNames';
import { BrandMark } from '../components/BrandMark';
import { Glyph } from '../components/Glyph';
import { gradientFor } from '../lib/catalog';

interface Page {
  title: string; artist: string | null; release_date: string | null; release_time?: string | null; released: boolean; cover: boolean;
  links: { platform: string; url: string }[];
  /** 아티스트가 켠 경우만 — 곡 순서대로 */
  lyrics?: { title: string; lyrics: string }[];
}

/** 발매 전 — 'D-3 · 12:04:31'이 1초마다 줄어들고, 0이 되면 onDone(페이지를 다시 읽어 '지금 들을 수 있어요'로) */
function Countdown({ date, time, onDone }: { date: string; time?: string | null; onDone: () => void }) {
  const target = releaseMoment(date, time);
  const [label, setLabel] = useState(() => countdownLabel(target));
  useEffect(() => {
    const t = window.setInterval(() => {
      const next = countdownLabel(target);
      setLabel(next);
      if (!next) { window.clearInterval(t); onDone(); }
    }, 1000);
    return () => window.clearInterval(t);
  }, [target]); // eslint-disable-line react-hooks/exhaustive-deps
  if (!label) return null;
  return (
    <div className="aq-smartlink-countdown" role="timer" aria-label={`발매까지 ${label}`}>
      <small>{Number(date.slice(5, 7))}월 {Number(date.slice(8, 10))}일{time ? ` ${time}` : ''} 발매까지</small>
      <strong aria-hidden="true">{label}</strong>
    </div>
  );
}

const MOCK_PAGE: Page = {
  title: '여름의 끝', artist: '한결', release_date: '2026-08-01', released: true, cover: false,
  lyrics: [{ title: '여름의 끝', lyrics: '바람이 식어 가는 저녁\n우리는 아무 말 없이 걸었지' }],
  links: [{ platform: 'melon', url: 'https://www.melon.com/' }, { platform: 'spotify', url: 'https://open.spotify.com/' }, { platform: 'apple', url: 'https://music.apple.com/' }, { platform: 'youtube', url: 'https://music.youtube.com/' }],
};

export function SmartLink() {
  const { slug = '' } = useParams<{ slug: string }>();
  // 아티스트 소개 페이지에서 눌러 들어왔으면 돌아가는 길을 위에 둔다
  const from = (useLocation().state as { artist?: { path: string; name: string } } | null)?.artist;
  const [page, setPage] = useState<Page | null>(null);
  const [missing, setMissing] = useState(false);
  const [coverOk, setCoverOk] = useState(true);
  const [email, setEmail] = useState('');
  const [agree, setAgree] = useState(false);
  const [alertState, setAlertState] = useState<'idle' | 'busy' | 'done'>('idle');
  const [alertError, setAlertError] = useState('');
  // 카운트다운이 끝나면 다시 읽어 발매된 화면으로
  const [reloadKey, setReloadKey] = useState(0);
  const base = `/api/smart-links/${encodeURIComponent(slug)}`;

  useEffect(() => {
    // 체험 모드: 'soon'으로 시작하는 코드는 발매 전 화면
    if (MOCK) { setPage(slug.startsWith('soon') ? { ...MOCK_PAGE, released: false, release_date: '2099-01-01' } : MOCK_PAGE); return; }
    req<Page>(base, { quiet401: true }).then(setPage, () => setMissing(true));
  }, [base, slug, reloadKey]);
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
        {from && <Link to={from.path} className="aq-smartlink-back"><Glyph name="chevron-left" size={16} />{from.name}</Link>}
        {cover ? <img className="aq-smartlink-cover" src={cover} alt={`${page.title} 커버`} onError={() => setCoverOk(false)} />
          // 커버가 없어도 빈 회색 사각형 대신 스튜디오와 같은 발매별 그라데이션 커버 — 팬이 처음 보는 화면이 비어 보였다
          : <div className="aq-smartlink-cover is-empty" aria-hidden="true" style={{ background: gradientFor(slug) }}><Glyph name="music" className="aq-cover-glyph" /></div>}
        <h1>{page.title}</h1>
        {page.artist && <p className="aq-smartlink-artist">{page.artist}</p>}
        {!page.released && page.release_date && (
          <Countdown date={page.release_date} time={page.release_time} onDone={() => setReloadKey(k => k + 1)} />
        )}
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
          <section className="aq-smartlink-links" aria-labelledby="slListTitle">
            <h2 id="slListTitle" className="aq-smartlink-list-title">{page.released ? '어디서 들을까요?' : '플랫폼에서 미리 보기'}</h2>
            <ul className="aq-smartlink-list">
              {page.links.map(l => {
                const name = PLATFORM_NAMES[l.platform] ?? l.platform;
                return (
                  <li key={l.platform}>
                    <a href={l.url} target="_blank" rel="noopener noreferrer nofollow" onClick={() => open(l.platform)}
                      aria-label={`${name}에서 ${page.released ? '듣기' : '보기'}`}>
                      <BrandMark id={l.platform} label={name} size={44} className="aq-smartlink-logo" />
                      <span className="aq-smartlink-name">
                        <strong>{name}</strong>
                        <small>{page.released ? '지금 바로 들을 수 있어요' : '발매 예정 페이지'}</small>
                      </span>
                      <span className="aq-smartlink-go">
                        {page.released ? '들으러 가기' : '보러 가기'}
                        <Glyph name="chevron-right" size={14} />
                      </span>
                    </a>
                  </li>
                );
              })}
            </ul>
          </section>
        ) : <p className="muted small">아직 등록된 플랫폼 링크가 없어요.</p>}
        {(page.lyrics?.length ?? 0) > 0 && (
          <section className="aq-smartlink-lyrics" aria-labelledby="slLyricsTitle">
            <h2 id="slLyricsTitle" className="aq-smartlink-list-title">가사</h2>
            {page.lyrics!.map((t, i) => (
              <details key={i} className="aq-faq-item" open={page.lyrics!.length === 1}>
                <summary>{t.title}<span className="aq-faq-chev" aria-hidden="true" /></summary>
                <p>{t.lyrics}</p>
              </details>
            ))}
          </section>
        )}
        <p className="aq-smartlink-foot">Distributed by AUDENIQ</p>
      </div>
    </main>
  );
}
