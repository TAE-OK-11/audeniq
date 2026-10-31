// 공동 작곡 합의서 (/works/:id/split-sheet) — 곡에 적은 참여자·역할·지분으로 바로 만드는 서류.
// 담당자가 보완으로 '공동 작곡 합의서'를 요청하거나, 함께 쓴 사람과 몫을 서류로 남기고 싶을 때 따로 만들지 않아도 된다.
// 신청서와 같은 종이 모양(aq-paper) — 인쇄·PDF로 저장해 참여자가 서명하고, 서명본은 '권리·보완 서류'에서 올린다.
import { useNavigate, useParams } from '../lib/router';
import { pubApi } from '../api/publishing';
import { useAsync } from '../hooks/useAsync';
import { localStamp } from '../lib/format';
import { WORK_TYPES, pct, roleTextOf, societyName, splitReady, sumShares } from '../lib/publishing';
import { SkeletonBlock } from '../components/Skeleton';
import { Glyph } from '../components/Glyph';
import { Callout } from '../ui';
import '../styles/split/Publishing.css';

export function SplitSheet() {
  const { id = '' } = useParams();
  const nav = useNavigate();
  const { data: w, error } = useAsync(() => pubApi.getWork(id), [id], { key: `work:${id}` });
  if (!w && !error) return <div className="view aq-doc-view"><SkeletonBlock height={640} /></div>;
  if (error || !w) {
    return (
      <div className="ui-empty">
        <h2>합의서를 만들 수 없어요.</h2>
        <p>{error || '곡을 찾을 수 없어요.'}</p>
        <button type="button" className="ui-btn is-soft" onClick={() => nav('/releases?tab=works')}>곡 목록</button>
      </div>
    );
  }
  const today = localStamp(new Date().toISOString()).replace(/ 오[전후].*$/, '');
  const ready = splitReady(w.contributors);
  const rec = w.recordings[0];
  return (
    <div id="view-split-sheet" className="view aq-doc-view">
      <div className="aq-doc-toolbar">
        <button type="button" className="ui-btn is-white is-sm aq-back-link" onClick={() => nav(`/works/${encodeURIComponent(w.id)}`)}>
          <Glyph name="arrow-left" size={15} className="aq-glyph-lead" />곡 화면
        </button>
        <div className="row-actions">
          <button type="button" className="ui-btn is-primary" onClick={() => window.print()}>인쇄 · PDF 저장</button>
        </div>
      </div>

      {!ready && <Callout tone="warn">지분 합계가 {pct(sumShares(w.contributors))}예요. 100%로 맞춘 뒤 합의서를 만들어 주세요.</Callout>}
      <p className="ui-field-help aq-pbl-demo-note">인쇄하거나 PDF로 저장해 참여자 모두 서명한 뒤, 서명본을 ‘권리·보완 서류’에 올려 주세요.</p>

      <article className="aq-paper" aria-label="공동 작곡 합의서">
        <header className="aq-paper-head">
          <div className="aq-paper-brand">
            <img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" />
            <dl className="aq-paper-meta">
              <div><dt>곡 코드</dt><dd>{w.iswc ?? w.internalCode}</dd></div>
              <div><dt>작성일</dt><dd>{today}</dd></div>
            </dl>
          </div>
          <h1>공동 작곡 합의서</h1>
          <p className="aq-paper-en">Songwriter Split Agreement</p>
        </header>

        <section className="aq-paper-sec">
          <h2><span>1</span>대상 곡</h2>
          <table className="aq-paper-kv"><tbody>
            <tr><th>곡명</th><td>{w.title}</td><th>곡 유형</th><td>{WORK_TYPES.find(x => x[0] === w.workType)?.[1] ?? w.workType}</td></tr>
            <tr><th>발매</th><td colSpan={3}>{rec ? `${rec.releaseTitle}${rec.isrc ? ` · ISRC ${rec.isrc}` : ''}` : w.externalRelease ? `${w.externalRelease.artist} · ${w.externalRelease.title}` : '발매 전'}</td></tr>
          </tbody></table>
        </section>

        <section className="aq-paper-sec">
          <h2><span>2</span>참여자와 지분 <small>{w.contributors.length}명 · 합계 {pct(sumShares(w.contributors))}</small></h2>
          <div className="aq-paper-scroll">
            <table className="aq-paper-grid">
              <thead><tr><th>No.</th><th>이름</th><th>역할</th><th>관리 단체 · IPI</th><th>창작 지분</th></tr></thead>
              <tbody>
                {w.contributors.map((c, i) => (
                  <tr key={c.id}>
                    <td className="num aq-paper-no">{i + 1}</td>
                    <td className="aq-paper-title"><strong>{c.name}</strong></td>
                    <td data-label="역할">{roleTextOf(c.roles)}</td>
                    <td data-label="단체·IPI">{[societyName(c.society), c.ipi && `IPI ${c.ipi}`].filter(Boolean).join(' · ') || '—'}</td>
                    <td data-label="창작 지분" className="num">{pct(c.share)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>

        <section className="aq-paper-sec">
          <h2><span>3</span>합의 내용</h2>
          <ol className="aq-paper-agree">
            <li>위 참여자는 대상 곡을 함께 창작했으며, 각자의 창작 지분은 위 표와 같다.</li>
            <li>대상 곡에서 생기는 저작권 사용료(공연·복제·전송 등)는 위 지분대로 나눈다.</li>
            <li>지분을 바꾸려면 참여자 모두가 서면으로 합의해야 한다.</li>
            <li>각 참여자는 자기 몫을 직접 또는 자신이 정한 관리 단체·퍼블리셔를 통해 관리할 수 있다.</li>
          </ol>
        </section>

        <section className="aq-paper-signoff">
          <p>위 내용에 합의하며 각자 서명합니다.</p>
          <p className="aq-paper-date">{today}</p>
          <div className="aq-split-signs">
            {w.contributors.map(c => (
              <div key={c.id} className="aq-split-sign">
                <span>{c.name} · {pct(c.share)}</span>
                <i aria-hidden="true" />
                <small>(서명)</small>
              </div>
            ))}
          </div>
        </section>

        <footer className="aq-paper-foot">
          <p>이 합의서는 AUDENIQ STUDIO에 등록한 곡 정보로 만들었어요. 참여자 모두 서명해야 효력이 있어요.</p>
        </footer>
      </article>
    </div>
  );
}
