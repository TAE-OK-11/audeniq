// 저작권 관리 신청서 (/publishing/applications/:id) — 배급 신청서(/releases/:id/application)와 같은 정식 서류.
// 접수 직후(?done=1)에는 맨 위에 '신청했어요' 카드와 남은 단계, 그 아래 서명한 신청서.
// 신청서는 신청 내용이 맞다는 서명이고, 계약은 담당자가 승인한 뒤 계약서 화면에서 따로 맺는다 (배급과 같다).
import { printWithStamp } from '../lib/printStamp';
import { useNavigate, useParams, useSearchParams } from '../lib/router';
import { pubApi } from '../api/publishing';
import { errorMessage } from '../api/errors';
import { useAsync } from '../hooks/useAsync';
import { MOCK } from '../lib/mode';
import { localStamp } from '../lib/format';
import { hashLabel } from '../lib/application';
import { APPLICATION_AGREE, APP_LABEL, APP_TONE, REG_STEPS, pct, rightName, roleTextOf, termLabel, territoryName } from '../lib/publishing';
import { useToast } from '../components/Toast';
import { SkeletonBlock } from '../components/Skeleton';
import { CheckIcon } from '../components/Check';
import { Glyph } from '../components/Glyph';
import { Burst } from '../components/Burst';
import { Badge } from '../ui';
import '../styles/split/Publishing.css';

export function PublishingApplicationDoc() {
  const { id = '' } = useParams();
  const [params] = useSearchParams();
  const done = params.get('done') === '1';
  const nav = useNavigate();
  const toast = useToast();
  const { data: apps, error, reload } = useAsync(() => pubApi.listApplications(), [], { key: 'pub-applications' });
  const { data: works } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  const { data: policy } = useAsync(() => pubApi.policy(), [], { key: 'pub-policy' });
  const app = apps?.find(a => a.id === id);

  if (!apps && !error) return <div className="view aq-doc-view"><SkeletonBlock height={640} /></div>;
  if (error || !app) {
    return (
      <div className="ui-empty">
        <h2>신청서를 확인할 수 없어요.</h2>
        <p>{error || '이 계정에서 볼 수 있는 저작권 관리 신청서가 없어요.'}</p>
        <button type="button" className="ui-btn is-soft" onClick={() => nav('/releases?tab=works')}>곡 목록</button>
      </div>
    );
  }

  const applied = app.workIds.map(wid => works?.find(w => w.id === wid)).filter(Boolean) as NonNullable<typeof works>[number][];
  const firstTitle = app.workTitles[0] ?? '곡';
  const approve = async () => {
    try { await pubApi.simulateApprove(app.id); reload(); toast('체험: 담당자가 승인했어요. 계약서가 만들어졌어요.', 'success'); }
    catch (e) { toast(errorMessage(e), 'error'); }
  };

  return (
    <div id="view-pub-application" className="view aq-doc-view">
      <div className="aq-doc-toolbar">
        <button type="button" className="ui-btn is-white is-sm aq-back-link" onClick={() => nav(`/works/${encodeURIComponent(app.workIds[0])}?tab=register`)}>
          <Glyph name="arrow-left" size={15} className="aq-glyph-lead" />곡 화면
        </button>
        <div className="row-actions">
          <button type="button" className="ui-btn is-soft" onClick={() => void printWithStamp({ kind: 'APPLICATION', ref: id, root: document.querySelector('.aq-paper') })}>인쇄 · PDF 저장</button>
          {app.contractId
            ? <button type="button" className="ui-btn is-primary" onClick={() => nav(`/contracts/publishing/${encodeURIComponent(app.contractId!)}`)}>계약서 보기</button>
            : done && <button type="button" className="ui-btn is-primary" onClick={() => nav('/')}>홈으로 가기</button>}
        </div>
      </div>

      {done && (
        <div className="aq-doc-done" role="status">
          <span className="aq-doc-done-mark" aria-hidden="true">
            <svg viewBox="0 0 24 24" width="22" height="22"><path d="M5 12.5l4.5 4.5L19 7.5" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round" /></svg>
            <Burst />
          </span>
          <div className="min-0">
            {/* 무엇을 했는지와 남은 단계 — 단계 이름은 곡 화면의 진행 단계(REG_STEPS)와 같은 말 */}
            <strong>{app.workTitles.length > 1 ? `‘${firstTitle}’ 외 ${app.workTitles.length - 1}곡` : `‘${firstTitle}’`} 저작권 관리를 신청했어요</strong>
            <span>서명한 신청서를 발급했어요. 남은 단계는 이래요.</span>
            <ol className="aq-doc-next" aria-label="다음 단계">
              <li className="is-done"><i aria-hidden="true"><CheckIcon size={10} /></i>{REG_STEPS[0]} — 신청서 서명, 방금 마쳤어요</li>
              <li><i aria-hidden="true">2</i>{REG_STEPS[1]} — 담당자가 참여자·지분을 확인해요. 보완이 필요하면 곡 화면에 보여요</li>
              <li><i aria-hidden="true">3</i>계약서 서명 — 승인되면 계약서 화면에서 해요</li>
              <li><i aria-hidden="true">4</i>{REG_STEPS[3]} · {REG_STEPS[4]} — 관리 단체 등록이 끝나야 저작권료를 받아요</li>
            </ol>
          </div>
        </div>
      )}

      {MOCK && ['submitted', 'in_review'].includes(app.status) && (
        <p className="ui-field-help aq-pbl-demo-note">
          체험 모드예요. <button type="button" className="ui-btn is-text is-sm" onClick={() => void approve()}>체험: 담당자 승인</button>을 누르면 계약서가 만들어져요.
        </p>
      )}

      <article className="aq-paper" aria-label="저작권 관리 신청서">
        <header className="aq-paper-head">
          <div className="aq-paper-brand">
            <img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" />
            <dl className="aq-paper-meta">
              <div><dt>신청서 번호</dt><dd>{app.number}</dd></div>
              <div><dt>접수 일시</dt><dd>{app.submittedAt ? localStamp(app.submittedAt) : '—'}</dd></div>
              <div><dt>상태</dt><dd><Badge tone={APP_TONE[app.status]}>{APP_LABEL[app.status]}</Badge></dd></div>
            </dl>
          </div>
          <h1>저작권 관리 신청서</h1>
          <p className="aq-paper-en">Music Publishing Administration Application</p>
        </header>

        <section className="aq-paper-sec">
          <h2><span>1</span>신청인</h2>
          <table className="aq-paper-kv"><tbody>
            <tr><th>성명</th><td>{app.signerName || '—'}</td><th>구분</th><td>저작권자</td></tr>
          </tbody></table>
        </section>

        <section className="aq-paper-sec">
          <h2><span>2</span>신청한 곡 <small>{app.workTitles.length}곡</small></h2>
          <div className="aq-paper-scroll">
            <table className="aq-paper-grid">
              <thead><tr><th>No.</th><th>곡명</th><th>참여자 · 지분</th><th>곡 코드</th></tr></thead>
              <tbody>
                {app.workIds.map((wid, i) => {
                  const w = applied.find(x => x.id === wid);
                  return (
                    <tr key={wid}>
                      <td className="num aq-paper-no">{i + 1}</td>
                      <td className="aq-paper-title"><strong>{app.workTitles[i] ?? w?.title}</strong></td>
                      <td data-label="참여자 · 지분">{w ? w.contributors.map(c => `${c.name}(${roleTextOf(c.roles)}) ${pct(c.share)}`).join(', ') : '—'}</td>
                      <td data-label="곡 코드">{w ? (w.iswc ?? w.internalCode) : '—'}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </section>

        <section className="aq-paper-sec">
          <h2><span>3</span>신청 조건</h2>
          <table className="aq-paper-kv"><tbody>
            <tr><th>관리 대상 국가</th><td colSpan={3}>{app.territories.map(territoryName).join(', ')}</td></tr>
            <tr><th>관리할 권리</th><td colSpan={3}>{app.rights.map(rightName).join(', ')}</td></tr>
            {policy && <tr><th>관리 수수료</th><td>징수한 사용료의 {policy.commissionRate}%</td><th>최초 계약 기간</th><td>{termLabel(policy.initialTermMonths)}</td></tr>}
          </tbody></table>
          <p className="aq-paper-p">계약 조건은 담당자가 신청 내용을 확인하고 승인할 때 확정되며, 확정된 조건으로 만든 저작권 관리 계약서에 따로 서명해야 계약이 맺어져요.</p>
        </section>

        <section className="aq-paper-sec">
          <h2><span>4</span>신청인 확인</h2>
          <ol className="aq-paper-agree">
            {APPLICATION_AGREE.map(([k, t]) => (
              <li key={k} className="is-on">
                <span className="aq-paper-check" role="img" aria-label="확인함"><CheckIcon size={12} /></span>
                <span><b className="aq-paper-req">필수</b> {t}</span>
              </li>
            ))}
          </ol>
        </section>

        <section className="aq-paper-signoff">
          <p>위 내용으로 저작권 관리를 신청합니다.</p>
          <p className="aq-paper-date">{app.submittedAt ? localStamp(app.submittedAt) : '—'}</p>
          <div className="aq-paper-record">
            <h3>전자서명 기록</h3>
            <dl>
              <div><dt>신청인</dt><dd>{app.signerName || '—'}</dd></div>
              <div><dt>서명 일시</dt><dd>{app.submittedAt ? localStamp(app.submittedAt) : '—'}</dd></div>
              <div><dt>서명 방법</dt><dd>AUDENIQ STUDIO 로그인 계정에서 손글씨 전자서명 · 확인 항목 {APPLICATION_AGREE.length}개 직접 체크</dd></div>
            </dl>
          </div>
        </section>

        <footer className="aq-paper-foot">
          <div>
            <span>문서 확인 코드 (SHA-256)</span>
            {app.documentCode ? <code>{hashLabel(app.documentCode)}</code> : <em className="aq-paper-pending">{MOCK ? '체험 모드에서는 발급하지 않아요' : '—'}</em>}
          </div>
          <p>이 신청서는 AUDENIQ STUDIO에서 전자문서로 작성되고 전자서명으로 접수됐어요. 신청서는 계약이 아니에요 — 계약은 승인 뒤 저작권 관리 계약서에 서명할 때 맺어져요.</p>
        </footer>
      </article>
    </div>
  );
}
