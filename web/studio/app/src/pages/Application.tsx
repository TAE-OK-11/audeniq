// 배급 신청서 — 신청인이 서명해 접수한 내용을 정식 서류 형태로 보여 주고 인쇄·PDF로 저장한다.
import { useEffect, useState } from 'react';
import { useNavigate, useParams, useSearchParams } from '../lib/router';
import { api } from '../api/client';
import { useAsync } from '../hooks/useAsync';
import { SkeletonBlock } from '../components/Skeleton';
import { useProfile } from '../store/profile';
import { dspLabel, genreLabel, kindLabel, languageLabel } from '../lib/catalog';
import { formatKoreanDate, parseStamp } from '../lib/date';
import { localStamp } from '../lib/format';
import { AGREEMENTS, OPTION_LABELS, hashLabel, snapshotFromRelease, verifyApplication } from '../lib/application';
import { PROFILE_LINKS } from '../lib/dsp';

type Integrity = 'checking' | 'ok' | 'changed';

const dash = (v?: string | null) => (v && v.trim() ? v : '—');

function longDate(stamp: string): string {
  const d = parseStamp(stamp);
  return d ? `${d.getFullYear()}년 ${d.getMonth() + 1}월 ${d.getDate()}일` : stamp;
}

export function Application() {
  const { id = '' } = useParams();
  const [params] = useSearchParams();
  const done = params.get('done') === '1';
  const nav = useNavigate();
  const profile = useProfile();
  const { data: rel, loading, error, reload } = useAsync(() => api.getRelease(id), [id]);
  const app = rel?.draft?.application;
  const [integrity, setIntegrity] = useState<Integrity>('checking');

  useEffect(() => {
    if (!rel || !app) return;
    let live = true;
    setIntegrity('checking');
    verifyApplication(app, rel).then(ok => { if (live) setIntegrity(ok ? 'ok' : 'changed'); }).catch(() => { if (live) setIntegrity('changed'); });
    return () => { live = false; };
  }, [rel, app]);

  if (loading) {
    return <div className="view aq-doc-view"><SkeletonBlock height={640} /></div>;
  }
  if (error || !rel) {
    return (
      <div className="empty-page">
        <h2>신청서를 불러오지 못했어요.</h2>
        <p>{error || '발매를 찾을 수 없어요.'}</p>
        <button type="button" className="button secondary" onClick={reload}>다시 시도</button>
      </div>
    );
  }
  if (!app) {
    return (
      <div className="empty-page">
        <h2>서명된 신청서가 없어요.</h2>
        <p>발매 신청서는 신청서 마지막 단계에서 서명하고 접수하면 발급돼요.</p>
        <button type="button" className="button" onClick={() => nav(`/releases/${rel.id}`)}>발매 상세로</button>
      </div>
    );
  }

  const snap = snapshotFromRelease(rel);
  const d = rel.draft;
  const cover = d?.coverData || rel.coverData;

  return (
    <div id="view-application" className="view aq-doc-view">
      {done && (
        <div className="aq-doc-done" role="status">
          <span className="aq-doc-done-mark" aria-hidden="true">
            <svg viewBox="0 0 24 24" width="22" height="22"><path d="M5 12.5l4.5 4.5L19 7.5" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round" /></svg>
          </span>
          <div className="min-0">
            <strong>배급 신청이 접수됐어요.</strong>
            <span>서명한 신청서가 발급됐어요. 담당자 검토가 끝나면 알림으로 알려 드릴게요.</span>
          </div>
        </div>
      )}

      <div className="aq-doc-toolbar">
        <button type="button" className="link-btn aq-back-link" onClick={() => nav(`/releases/${rel.id}`)}>← 발매 상세</button>
        <div className="row-actions">
          <button type="button" className="button secondary" onClick={() => nav('/')}>홈으로</button>
          <button type="button" className="button" onClick={() => window.print()}>인쇄 · PDF 저장</button>
        </div>
      </div>

      <article className="aq-paper" aria-label="디지털 음원 배급 신청서">
        <header className="aq-paper-head">
          <div className="aq-paper-brand">
            <img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" />
            <dl className="aq-paper-meta">
              <div><dt>신청서 번호</dt><dd>{app.no}</dd></div>
              <div><dt>접수 일시</dt><dd>{localStamp(app.submittedAt)}</dd></div>
              <div><dt>서식</dt><dd>{app.form}</dd></div>
            </dl>
          </div>
          <h1>디지털 음원 배급 신청서</h1>
          <p className="aq-paper-en">Digital Music Distribution Application</p>
        </header>

        <section className="aq-paper-sec">
          <h2><span>1</span>신청인</h2>
          <table className="aq-paper-kv">
            <tbody>
              <tr><th>성명</th><td>{app.signerName}</td><th>구분</th><td>{app.signerRole}</td></tr>
              <tr><th>대표 아티스트</th><td>{dash(snap.artist)}</td><th>연락 이메일</th><td className="break">{dash(profile.email)}</td></tr>
            </tbody>
          </table>
        </section>

        <section className="aq-paper-sec">
          <h2><span>2</span>발매 정보</h2>
          <div className="aq-paper-release">
            {cover && <img className="aq-paper-cover" src={cover} alt="커버아트" />}
            <table className="aq-paper-kv">
              <tbody>
                <tr><th>발매 제목</th><td colSpan={3}><strong>{snap.title}</strong></td></tr>
                <tr><th>발매 유형</th><td>{kindLabel(snap.type)}</td><th>장르</th><td>{genreLabel(snap.genre) || dash(snap.genre)}</td></tr>
                <tr><th>주요 언어</th><td>{languageLabel(snap.language) || dash(snap.language)}</td><th>레이블 표기</th><td>{dash(snap.label)}</td></tr>
                <tr><th>발매 예정일</th><td>{snap.releaseDate ? formatKoreanDate(snap.releaseDate) : '—'}</td><th>UPC / EAN</th><td>{snap.upc || '발급 예정'}</td></tr>
                {snap.originalDate && <tr><th>최초 발매일</th><td colSpan={3}>{formatKoreanDate(snap.originalDate)}</td></tr>}
                <tr>
                  <th>플랫폼 프로필</th>
                  <td colSpan={3} className="break">
                    {snap.artistProfile.isNew
                      ? '신규 아티스트 (새 프로필 생성)'
                      : PROFILE_LINKS.filter(l => snap.artistProfile[l.key]).map(l => `${l.label}: ${snap.artistProfile[l.key]}`).join(' / ')}
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </section>

        <section className="aq-paper-sec">
          <h2><span>3</span>수록곡 <small>{snap.tracks.length}곡</small></h2>
          <div className="aq-paper-scroll">
            <table className="aq-paper-grid">
              <thead>
                <tr><th>No.</th><th>곡명</th><th>작곡</th><th>작사</th><th>길이</th><th>음원 · ISRC</th></tr>
              </thead>
              <tbody>
                {snap.tracks.map((t, i) => (
                  <tr key={i}>
                    <td className="num">{i + 1}</td>
                    <td>
                      <strong>{t.title}</strong>{t.version && ` (${t.version})`}
                      {t.explicit && <em className="aq-paper-tag">19</em>}
                      {t.featuring && <small>피처링 {t.featuring}</small>}
                      {(t.arrangers || t.producer) && <small>{[t.arrangers && `편곡 ${t.arrangers}`, t.producer && `프로듀서 ${t.producer}`].filter(Boolean).join(' · ')}</small>}
                    </td>
                    <td>{t.composers}</td>
                    <td>{t.instrumental ? '연주곡' : dash(t.lyricists)}</td>
                    <td className="num">{dash(t.duration)}</td>
                    <td>
                      <small>{t.audioSpec || t.audioName || '—'}</small>
                      <small>{t.isrc ? `ISRC ${t.isrc}` : 'ISRC 발급 예정'}</small>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>

        <section className="aq-paper-sec">
          <h2><span>4</span>배급 범위 및 권리</h2>
          <table className="aq-paper-kv">
            <tbody>
              <tr><th>배급 지역</th><td colSpan={3}>{snap.territories.includes('WORLD') ? '전 세계' : dash(snap.territories.join(', '))}</td></tr>
              <tr><th>배급 플랫폼</th><td colSpan={3}>{snap.platforms.map(dspLabel).join(', ') || '—'}</td></tr>
              <tr><th>마스터 권리자</th><td colSpan={3}>{dash(snap.ownership)}</td></tr>
              <tr><th>℗ 표기</th><td>℗ {dash(snap.phonogram)}</td><th>© 표기</th><td>© {dash(snap.copyright)}</td></tr>
              <tr><th>신고 항목</th><td colSpan={3}>{snap.options.map(k => OPTION_LABELS[k] ?? k).join(', ') || '해당 없음 (일반 발매)'}</td></tr>
            </tbody>
          </table>
        </section>

        <section className="aq-paper-sec">
          <h2><span>5</span>신청인 확인 및 동의</h2>
          <ol className="aq-paper-agree">
            {AGREEMENTS.map(a => (
              <li key={a.id} className={app.agreements.includes(a.id) ? 'is-on' : ''}>
                <span className="aq-paper-check" aria-label={app.agreements.includes(a.id) ? '동의함' : '동의하지 않음'}>
                  {app.agreements.includes(a.id) ? '✓' : ''}
                </span>
                {a.text}
              </li>
            ))}
          </ol>
        </section>

        <section className="aq-paper-signoff">
          <p>위와 같이 AUDENIQ 디지털 음원 배급을 신청합니다.</p>
          <p className="aq-paper-date">{longDate(app.submittedAt)}</p>
          <div className="aq-paper-signer">
            <span>신청인</span>
            <strong>{app.signerName}</strong>
            <span className="aq-paper-sig">
              {app.signature ? <img src={app.signature} alt={`${app.signerName} 서명`} /> : <em>(서명)</em>}
            </span>
          </div>
          <p className="aq-paper-to">AUDENIQ 귀중</p>
        </section>

        <footer className="aq-paper-foot">
          <div>
            <span>문서 확인 코드 (SHA-256)</span>
            <code>{hashLabel(app.hash)}</code>
          </div>
          <span className={`aq-paper-integrity is-${integrity}`}>
            {integrity === 'ok' ? '접수 원본과 일치' : integrity === 'changed' ? '접수 후 내용이 바뀌었어요' : '원본 확인 중'}
          </span>
          <p>이 신청서는 AUDENIQ STUDIO에서 전자서명으로 작성됐어요. 문서 확인 코드는 신청 내용과 서명으로 계산돼, 내용이 바뀌면 달라져요.</p>
        </footer>
      </article>
    </div>
  );
}
