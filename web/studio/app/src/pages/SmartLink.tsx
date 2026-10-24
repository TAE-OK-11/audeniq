// 스마트링크 (/link/release/:slug) — 로그인 없이 누구나. 발매 하나의 공유 페이지: 커버·제목·아티스트와 플랫폼별 듣기 링크.
// 아티스트가 발매 상세에서 링크를 넣고 켜 둔 동안만 보인다. 조회·클릭 수는 아티스트에게 보인다.
import { useEffect, useState } from 'react';
import { countdownLabel, releaseMoment } from '../lib/countdown';
import { preReleaseAction } from '../lib/preRelease';
import { releaseUrl } from '../lib/publicLinks';
import { presaveResult, releaseIcs, withoutPresaveQuery, type PresaveResult } from '../lib/presave';
import { Link, useLocation, useParams } from '../lib/router';
import { API_BASE, req } from '../api/http';
import { errorMessage } from '../api/errors';
import { MOCK } from '../lib/mode';
import { PLATFORM_NAMES } from '../lib/platformNames';
import { BrandMark } from '../components/BrandMark';
import { Glyph } from '../components/Glyph';
import { gradientFor } from '../lib/catalog';
import { PublicFoot, PublicLoading, PublicMissing, PublicShell } from '../components/PublicPage';

interface Page {
  title: string; artist: string | null; release_date: string | null; release_time?: string | null; released: boolean; cover: boolean;
  links: { platform: string; url: string }[];
  /** 아티스트가 켠 경우만 — 곡 순서대로 */
  lyrics?: { title: string; lyrics: string }[];
  /** 발매 전에만 — 미리 저장할 수 있는 플랫폼 (운영 설정이 된 곳) */
  presave?: string[];
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

/** 발매 순간을 캘린더 일정 파일로 — 휴대폰에서 열면 캘린더 앱이 '추가'를 묻는다 */
function saveCalendar(page: Page, slug: string) {
  if (!page.release_date) return;
  const ics = releaseIcs({ title: page.title, artist: page.artist, date: page.release_date, time: page.release_time, url: releaseUrl(slug) });
  const url = URL.createObjectURL(new Blob([ics], { type: 'text/calendar;charset=utf-8' }));
  const a = document.createElement('a');
  a.href = url;
  a.download = `${page.title}-발매.ics`;
  document.body.appendChild(a);
  a.click();
  a.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
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
  // 플랫폼에서 돌아온 결과 — 한 번 보여 주고 주소에서는 지운다
  const [presaved, setPresaved] = useState<PresaveResult | null>(() => presaveResult(window.location.search));
  const [presaveBusy, setPresaveBusy] = useState('');
  useEffect(() => {
    if (presaved) window.history.replaceState(window.history.state, '', `${window.location.pathname}${withoutPresaveQuery(window.location.search)}${window.location.hash}`);
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
  const base = `/api/smart-links/${encodeURIComponent(slug)}`;

  useEffect(() => {
    // 체험 모드: 'soon'으로 시작하는 코드는 발매 전 화면
    if (MOCK) { setPage(slug.startsWith('soon') ? { ...MOCK_PAGE, released: false, release_date: '2099-01-01', presave: ['spotify', 'deezer', 'tidal'] } : MOCK_PAGE); return; }
    // 채널 표시(?src=instagram)가 붙어 들어왔으면 같이 보내 채널별 방문으로 센다
    const src = new URLSearchParams(window.location.search).get('src');
    const q = src && /^[a-z]{1,20}$/.test(src) ? `?src=${src}` : '';
    req<Page>(`${base}${q}`, { quiet401: true }).then(setPage, () => setMissing(true));
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

  // 플랫폼 로그인·허락 화면으로 — 돌아오면 ?presave= 로 결과가 붙는다
  const presave = async (platform: string) => {
    setPresaved(null);
    if (MOCK) { setPresaved(presaveResult(`?presave=${platform}`)); return; }
    setPresaveBusy(platform);
    try {
      const r = await req<{ url: string }>(`${base}/presave/${platform}`, { method: 'POST', body: {} });
      if (!/^https?:\/\//.test(r.url)) throw new Error('');
      window.location.assign(r.url);
    } catch (e) {
      setPresaved({ ok: false, text: errorMessage(e, '미리 저장을 시작하지 못했어요. 잠시 뒤 다시 시도해 주세요.') });
      setPresaveBusy('');
    }
  };

  if (missing) return <PublicMissing text="링크가 꺼졌거나 주소가 잘못됐어요." />;
  if (!page) return <PublicLoading hero="cover" />;
  const cover = page.cover && coverOk ? `${API_BASE}${base}/cover` : '';
  const fromArtist = from && /^\/link\/artist\/([^/]+)$/.exec(from.path)?.[1];

  return (
    <PublicShell bg={cover || undefined} className="aq-releasepage" cardClass="aq-smartlink-release">
        {from && <Link to={from.path} className="aq-smartlink-back"><Glyph name="chevron-left" size={16} />{from.name}</Link>}
        {cover ? <img className="aq-smartlink-cover" src={cover} alt={`${page.title} 커버`} onError={() => setCoverOk(false)} />
          // 커버가 없어도 빈 회색 사각형 대신 스튜디오와 같은 발매별 그라데이션 커버 — 팬이 처음 보는 화면이 비어 보였다
          : <div className="aq-smartlink-cover is-empty" aria-hidden="true" style={{ background: gradientFor(slug) }}><Glyph name="music" className="aq-cover-glyph" /></div>}
        <h1>{page.title}</h1>
        {page.artist && <p className="aq-smartlink-artist">{page.artist}</p>}
        {!page.released && page.release_date && (
          <>
            <Countdown date={page.release_date} time={page.release_time} onDone={() => setReloadKey(k => k + 1)} />
            <button type="button" className="ui-btn is-sm aq-smartlink-cal" onClick={() => saveCalendar(page, slug)}>
              <Glyph name="calendar" size={14} />캘린더에 추가
            </button>
          </>
        )}
        {presaved && (
          <p className={`aq-smartlink-presave-result${presaved.ok ? '' : ' is-error'}`} role={presaved.ok ? 'status' : 'alert'}>
            <Glyph name={presaved.ok ? 'check' : 'alert'} size={16} />{presaved.text}
          </p>
        )}
        {!page.released && (page.presave?.length ?? 0) > 0 && (
          <section className="aq-smartlink-presave" aria-labelledby="slPresaveTitle">
            <h2 id="slPresaveTitle" className="aq-smartlink-list-title">발매 전에 미리 저장</h2>
            <p className="aq-smartlink-presave-help">한 번 허락해 두면 발매되는 순간 내 보관함에 자동으로 들어가요.</p>
            <ul className="aq-smartlink-presave-list">
              {page.presave!.map(p => {
                const name = PLATFORM_NAMES[p] ?? p;
                return (
                  <li key={p}>
                    <button type="button" className="aq-smartlink-presave-btn" disabled={!!presaveBusy} aria-busy={presaveBusy === p}
                      onClick={() => void presave(p)} aria-label={`${name}에 미리 저장`}>
                      <BrandMark id={p} label={name} size={28} />
                      <span>{name}</span>
                      <small>{presaveBusy === p ? '여는 중' : '미리 저장'}</small>
                    </button>
                  </li>
                );
              })}
            </ul>
            <p className="aq-smartlink-presave-note">AUDENIQ는 계정 정보를 받지 않아요. 저장에만 쓰고, 저장하면 권한을 바로 지워요.</p>
          </section>
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
            <h2 id="slListTitle" className="aq-smartlink-list-title">{page.released ? '어디서 들을까요?' : page.links.some(l => { const a = preReleaseAction(l.platform); return a && a !== '미리 보기'; }) ? '앱에서 미리 저장하기' : '플랫폼에서 미리 보기'}</h2>
            <ul className="aq-smartlink-list">
              {page.links.map(l => {
                const name = PLATFORM_NAMES[l.platform] ?? l.platform;
                // 발매 전 — 그 앱의 발매 예정 페이지에서 미리 저장·추가 (플랫폼 사전 공개)
                const pre = page.released ? null : preReleaseAction(l.platform);
                return (
                  <li key={l.platform}>
                    <a href={l.url} target="_blank" rel="noopener noreferrer nofollow" onClick={() => open(l.platform)}
                      aria-label={`${name}에서 ${page.released ? '듣기' : pre ?? '보기'}`}>
                      <BrandMark id={l.platform} label={name} size={44} className="aq-smartlink-logo" />
                      <span className="aq-smartlink-name">
                        <strong>{name}</strong>
                        <small>{page.released ? '지금 바로 들을 수 있어요' : pre && pre !== '미리 보기' ? '발매되면 보관함에 들어가요' : '발매 예정 페이지'}</small>
                      </span>
                      <span className="aq-smartlink-go">
                        {page.released ? '들으러 가기' : pre && pre !== '미리 보기' ? pre : '보러 가기'}
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
        <PublicFoot artist={fromArtist ? { slug: fromArtist, name: from!.name, releases: true } : undefined} />
    </PublicShell>
  );
}
