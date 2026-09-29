// 배급 신청서 — 아티스트가 스튜디오에서 서명해 접수한 신청서를 스튜디오와 같은 정식 서류 모양으로 보여 준다.
// (web/studio/app/src/pages/Application.tsx와 같은 구성·클래스 aq-paper-*)
import type { ReleaseSheet } from '../api/staff';
import { dspLabel } from '../labels';
import {
  AGREEMENTS, OPTION_LABELS, PROFILE_LINKS, displayCode, genreLabel, hashLabel, kindLabel, languageLabel,
  type Integrity, type StudioDraft,
} from '../lib/application';
import { CheckIcon } from './Check';

const dash = (v?: string | null) => (v && v.trim() ? v : '—');

/** '2026-09-29 22:37' · ISO 모두 받는다 */
function stamp(v?: string | null): string {
  if (!v) return '—';
  const m = /^(\d{4})-(\d{2})-(\d{2})[ T](\d{2}):(\d{2})/.exec(v);
  return m ? `${m[1]}.${m[2]}.${m[3]} ${m[4]}:${m[5]}` : v;
}
function longDate(v?: string | null): string {
  const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(v ?? '');
  return m ? `${+m[1]}년 ${+m[2]}월 ${+m[3]}일` : dash(v);
}

export const INTEGRITY_LABEL: Record<Integrity, string> = {
  ok: '서명한 원본과 일치', changed: '서명 후 내용이 달라졌어요', unsigned: '전자서명 기록 없음', checking: '원본 확인 중',
};

export function ApplicationPaper({ sheet, draft, integrity }: { sheet: ReleaseSheet; draft: StudioDraft; integrity: Integrity }) {
  const app = draft.application;
  const signed = sheet.signed_application;
  const o = (draft.options ?? {}) as Record<string, unknown>;
  const options = Object.keys(OPTION_LABELS).filter(k => o[k] === true);
  const tracks = (draft.draftTracks ?? []).filter(t => t.title?.trim());
  const territories = draft.territories ?? [];
  const ap = draft.artistProfile;
  const no = app?.no ?? signed?.application_no ?? '';
  const signerName = app?.signerName ?? signed?.signer_name ?? '';
  const agreements = app?.agreements ?? signed?.agreements ?? [];

  return (
    <article className="aq-paper adm-paper" aria-label="디지털 음원 배급 신청서">
      <header className="aq-paper-head">
        <div className="aq-paper-brand">
          <img src="/static/AUDENIQ_Logo_Light.svg" alt="AUDENIQ" />
          <dl className="aq-paper-meta">
            <div><dt>신청서 번호</dt><dd>{no ? displayCode(no) : '—'}</dd></div>
            <div><dt>접수 일시</dt><dd>{stamp(app?.submittedAt ?? signed?.received_at)}</dd></div>
            <div><dt>서식</dt><dd>{displayCode(app?.form ?? signed?.form ?? '—')}</dd></div>
          </dl>
        </div>
        <h1>디지털 음원 배급 신청서</h1>
        <p className="aq-paper-en">Digital Music Distribution Application</p>
      </header>

      <section className="aq-paper-sec">
        <h2><span>1</span>신청인</h2>
        <table className="aq-paper-kv">
          <tbody>
            <tr><th>성명</th><td>{dash(signerName)}</td><th>구분</th><td>{dash(app?.signerRole ?? signed?.signer_role)}</td></tr>
            <tr><th>대표 아티스트</th><td>{dash(draft.artist)}</td><th>조직</th><td>{sheet.release.org_name}</td></tr>
          </tbody>
        </table>
      </section>

      <section className="aq-paper-sec">
        <h2><span>2</span>발매 정보</h2>
        <div className="aq-paper-release">
          {(draft.coverData || sheet.release.cover) && <img className="aq-paper-cover" src={draft.coverData || sheet.release.cover || ''} alt="커버아트" />}
          <table className="aq-paper-kv">
            <tbody>
              <tr><th>발매 제목</th><td colSpan={3}><strong>{sheet.release.title}</strong></td></tr>
              <tr><th>발매 유형</th><td>{kindLabel(draft.type) || '—'}</td><th>장르</th><td>{genreLabel(draft) || '—'}</td></tr>
              <tr><th>주요 언어</th><td>{languageLabel(draft.language) || '—'}</td><th>레이블 표기</th><td>{dash(draft.label)}</td></tr>
              <tr><th>발매 예정일</th><td>{draft.release_date ? longDate(draft.release_date) : '—'}</td><th>UPC / EAN</th><td>{draft.upc || sheet.release.upc || '발급 예정'}</td></tr>
              {draft.originalDate && <tr><th>최초 발매일</th><td colSpan={3}>{longDate(draft.originalDate)}</td></tr>}
              <tr>
                <th>플랫폼 프로필</th>
                <td colSpan={3} className="break">
                  {!ap || ap.isNew
                    ? '신규 아티스트 (새 프로필 생성)'
                    : PROFILE_LINKS.filter(l => ap[l.key]).map(l => `${l.label}: ${ap[l.key]}`).join(' / ') || '—'}
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </section>

      <section className="aq-paper-sec">
        <h2><span>3</span>수록곡 <small>{tracks.length}곡</small></h2>
        <div className="aq-paper-scroll">
          <table className="aq-paper-grid">
            <thead><tr><th>No.</th><th>곡명</th><th>작곡</th><th>작사</th><th>길이</th><th>음원 · ISRC</th></tr></thead>
            <tbody>
              {tracks.map((t, i) => (
                <tr key={t.id ?? i}>
                  <td className="num">{i + 1}</td>
                  <td>
                    <strong>{t.title}</strong>{t.version && ` (${t.version})`}
                    {t.explicit && <em className="aq-paper-tag">19</em>}
                    {t.featuring && <small>피처링 {t.featuring}</small>}
                    {(t.arrangers || t.producer) && <small>{[t.arrangers && `편곡 ${t.arrangers}`, t.producer && `프로듀서 ${t.producer}`].filter(Boolean).join(' · ')}</small>}
                  </td>
                  <td>{dash(t.composers)}</td>
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
            <tr><th>배급 지역</th><td colSpan={3}>{territories.includes('WORLD') ? '전 세계' : dash(territories.join(', '))}</td></tr>
            <tr><th>배급 플랫폼</th><td colSpan={3}>{(draft.platforms ?? []).map(dspLabel).join(', ') || '—'}</td></tr>
            <tr><th>마스터 권리자</th><td colSpan={3}>{dash(draft.ownership)}</td></tr>
            <tr><th>℗ 표기</th><td>℗ {dash(draft.phonogram)}</td><th>© 표기</th><td>© {dash(draft.copyright)}</td></tr>
            <tr><th>신고 항목</th><td colSpan={3}>{options.map(k => OPTION_LABELS[k] ?? k).join(', ') || '해당 없음 (일반 발매)'}</td></tr>
          </tbody>
        </table>
      </section>

      <section className="aq-paper-sec">
        <h2><span>5</span>신청인 확인 및 동의</h2>
        <ol className="aq-paper-agree">
          {AGREEMENTS.map(a => {
            const on = agreements.includes(a.id);
            return (
              <li key={a.id} className={on ? 'is-on' : ''}>
                <span className="aq-paper-check" aria-label={on ? '동의함' : '동의하지 않음'}>{on ? <CheckIcon size={12} /> : null}</span>
                {a.text}
              </li>
            );
          })}
        </ol>
      </section>

      <section className="aq-paper-signoff">
        <p>위와 같이 AUDENIQ 디지털 음원 배급을 신청합니다.</p>
        <p className="aq-paper-date">{longDate(app?.submittedAt ?? signed?.received_at)}</p>
        <div className="aq-paper-signer">
          <span>신청인</span>
          <strong>{dash(signerName)}</strong>
          <span className="aq-paper-sig">{app?.signature ? <img src={app.signature} alt={`${signerName} 서명`} /> : <em>(서명)</em>}</span>
        </div>
        <p className="aq-paper-to">AUDENIQ 귀중</p>
      </section>

      <footer className="aq-paper-foot">
        <div>
          <span>문서 확인 코드 (SHA-256)</span>
          <code>{hashLabel(app?.hash ?? signed?.content_hash ?? '')}</code>
        </div>
        <span className={`aq-paper-integrity is-${integrity === 'unsigned' ? 'legacy' : integrity}`}>{INTEGRITY_LABEL[integrity]}</span>
        <p>문서 확인 코드는 신청 내용과 서명으로 계산돼요. 심사 화면이 받은 제출 내용으로 다시 계산해 접수 때 기록된 코드와 비교했어요.</p>
      </footer>
    </article>
  );
}
