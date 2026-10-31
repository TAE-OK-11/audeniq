// 저작권 관리 계약서 (/contracts/publishing/:id) — 배급 계약서(/contracts/:id)와 같은 정식 서류.
// 담당자가 신청을 승인하면 확정된 조건으로 만들어지고, 아티스트가 읽고 확인 항목에 모두 체크한 뒤 서명하면 계약이 맺어진다.
// 보완은 여기서 하지 않는다 — 곡 화면에서 끝낸 뒤 승인되면 이 계약서가 나타난다.
import { useRef, useState } from 'react';
import { printWithStamp } from '../lib/printStamp';
import { useNavigate, useParams } from '../lib/router';
import { pubApi } from '../api/publishing';
import { errorMessage } from '../api/errors';
import { useAsync } from '../hooks/useAsync';
import { useAccount } from '../store/account';
import { useProfile } from '../store/profile';
import { compactSignature, hashLabel } from '../lib/application';
import { localStamp } from '../lib/format';
import { CONTRACT_CONFIRMATIONS, CONTRACT_LABEL, CONTRACT_TONE, rightName, termLabel, territoryName } from '../lib/publishing';
import { COMPANY, companyRows, companySeal } from '../lib/company';
import { useToast } from '../components/Toast';
import { SkeletonBlock } from '../components/Skeleton';
import { CheckIcon } from '../components/Check';
import { Glyph } from '../components/Glyph';
import { SignaturePad, type SignaturePadHandle } from '../components/SignaturePad';
import { Badge } from '../ui';
import { VerifyGate } from '../components/publishing/VerifyGate';
import '../styles/split/Upload.css';
import '../styles/split/Publishing.css';

export function PublishingAgreement() {
  const { id = '' } = useParams();
  const nav = useNavigate();
  const toast = useToast();
  const account = useAccount();
  const profile = useProfile();
  // 저작권 관리 계약은 본인인증한 실명으로만 — 활동명·필명으로는 맺지 않는다 (서버도 실명으로 기록한다)
  const signerName = account.verified ? account.realName.trim() : '';
  const { data: list, error, reload } = useAsync(() => pubApi.listContracts(), [], { key: 'pub-contracts' });
  const c = list?.find(x => x.id === id);
  const [ticked, setTicked] = useState<string[]>([]);
  const [drawn, setDrawn] = useState(false);
  const [busy, setBusy] = useState(false);
  const pad = useRef<SignaturePadHandle>(null);
  const signRef = useRef<HTMLElement>(null);

  if (!list && !error) return <div className="view aq-doc-view"><SkeletonBlock height={640} /></div>;
  if (error || !c) {
    return (
      <div className="ui-empty">
        <h2>계약서를 확인할 수 없어요.</h2>
        <p>{error || '이 계정에서 볼 수 있는 저작권 관리 계약서가 없어요.'}</p>
        <button type="button" className="ui-btn is-soft" onClick={() => nav('/contracts')}>계약서 목록</button>
      </div>
    );
  }

  const signed = c.status !== 'pending_sign';
  const items = c.confirmations?.length ? c.confirmations.map(x => [x.id, x.text] as const) : CONTRACT_CONFIRMATIONS;
  const all = items.every(([k]) => ticked.includes(k));
  const toggle = (k: string) => setTicked(t => (t.includes(k) ? t.filter(x => x !== k) : [...t, k]));
  const seal = companySeal();
  const articles = (c.body ?? '').split('\n').filter(Boolean).map(line => {
    const m = /^제(\d+)조\s*\(([^)]+)\)\s*(.*)$/.exec(line);
    return m ? { no: m[1], title: m[2], text: m[3] } : { no: '', title: '', text: line };
  });

  const sign = async () => {
    if (busy || !all || !drawn) return;
    setBusy(true);
    try {
      await pubApi.signContract(c.id, {
        signerName, confirmations: items.map(([k]) => k), rowVersion: c.rowVersion ?? 1,
        // 배급 계약서와 같게 — 서명은 작게 줄여 보낸다
        signature: await compactSignature(pad.current?.toDataURL() ?? ''),
      });
      reload();
      toast('계약을 맺었어요. 이제 관리 단체 등록을 진행해요.', 'success');
      window.scrollTo({ top: 0, behavior: 'smooth' });
    } catch (e) { toast(errorMessage(e, '서명하지 못했어요. 다시 시도해 주세요.'), 'error'); }
    finally { setBusy(false); }
  };

  return (
    <div id="view-pub-agreement" className="view aq-doc-view">
      <div className="aq-doc-toolbar">
        <button type="button" className="ui-btn is-white is-sm aq-back-link" onClick={() => nav('/contracts')}><Glyph name="arrow-left" size={15} className="aq-glyph-lead" />계약서</button>
        <div className="row-actions">
          {c.applicationId && <button type="button" className="ui-btn is-soft" onClick={() => nav(`/publishing/applications/${encodeURIComponent(c.applicationId!)}`)}>신청서 보기</button>}
          <button type="button" className="ui-btn is-soft" onClick={() => void printWithStamp({ kind: 'DOCUMENT', ref: id, root: document.querySelector('.aq-paper') })}>인쇄 · PDF 저장</button>
        </div>
      </div>

      {!signed && (
        <div className="aq-doc-done is-pending" role="status">
          <span className="aq-doc-done-mark" aria-hidden="true"><Glyph name="pencil" size={18} /></span>
          <div className="min-0">
            <strong>서명 전 계약서예요.</strong>
            <span>담당자가 신청 내용을 확인하고 조건을 확정했어요. 내용을 읽고 확인 항목에 체크한 뒤 서명하면 계약이 맺어져요.</span>
          </div>
          <button type="button" className="ui-btn is-primary" onClick={() => signRef.current?.scrollIntoView({ behavior: 'smooth', block: 'start' })}>확인하고 서명</button>
        </div>
      )}

      <article className="aq-paper" aria-label="저작권 관리 계약서">
        <header className="aq-paper-head">
          <div className="aq-paper-brand">
            <img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" />
            <dl className="aq-paper-meta">
              <div><dt>계약서 번호</dt><dd>{c.number}</dd></div>
              {c.applicationNumber && <div><dt>신청서 번호</dt><dd>{c.applicationNumber}</dd></div>}
              <div><dt>체결일</dt><dd>{c.signedAt ? localStamp(c.signedAt) : '서명 전'}</dd></div>
            </dl>
          </div>
          <h1>저작권 관리 계약서</h1>
          <p className="aq-paper-en">Music Publishing Administration Agreement</p>
        </header>

        <section className="aq-paper-sec">
          <h2>계약 조건</h2>
          <table className="aq-paper-kv"><tbody>
            <tr><th>대상 곡</th><td colSpan={3}>{c.works.map(w => w.title).join(', ')}</td></tr>
            <tr><th>관리 대상 국가</th><td colSpan={3}>{c.territories.map(territoryName).join(', ')}</td></tr>
            <tr><th>관리할 권리</th><td colSpan={3}>{c.rights.map(rightName).join(', ')}</td></tr>
            <tr><th>관리 수수료</th><td>징수한 사용료의 {c.commissionRate}%</td><th>계약 기간</th><td>{c.startDate ? `${c.startDate} ~ ${c.endDate}` : `서명한 날부터 ${termLabel(c.termMonths)}`}</td></tr>
            <tr><th>상태</th><td colSpan={3}><Badge tone={CONTRACT_TONE[c.status]}>{CONTRACT_LABEL[c.status]}</Badge></td></tr>
          </tbody></table>
        </section>

        {articles.map((a, i) => (
          <section key={i} className="aq-paper-sec">
            {a.no && <h2><span>{a.no}</span>제{a.no}조 {a.title}</h2>}
            <p className="aq-paper-p">{a.text}</p>
          </section>
        ))}

        <section className="aq-paper-sec" ref={signRef}>
          <h2>계약 확인</h2>
          {signed ? (
            <ol className="aq-paper-agree">
              {items.map(([k, t]) => (
                <li key={k} className="is-on">
                  <span className="aq-paper-check" role="img" aria-label="확인함"><CheckIcon size={12} /></span>
                  <span><b className="aq-paper-req">필수</b> {t}</span>
                </li>
              ))}
            </ol>
          ) : (
            <div className="aq-doc-sign">
              <div className="aq-agree-box">
                <label className="ui-check is-plain aq-agree-all-line">
                  <input type="checkbox" checked={all} onChange={e => setTicked(e.target.checked ? items.map(([k]) => k) : [])} />
                  <span><strong>아래 내용을 모두 확인했어요</strong></span>
                </label>
                {items.map(([k, t]) => (
                  <label key={k} className="ui-check is-plain">
                    <input type="checkbox" checked={ticked.includes(k)} onChange={() => toggle(k)} />
                    <span>{t}</span>
                  </label>
                ))}
              </div>
              {/* 본인인증 전에는 서명 대신 인증이 먼저 할 일 — 큰 버튼 하나만 */}
              {!signerName ? <VerifyGate what="계약" /> : <>
                <div className="ui-field">
                  <label htmlFor="pubContractPad">서명 <span className="required">*</span></label>
                  <SignaturePad ref={pad} id="pubContractPad" label="계약자 서명 입력" height={170} onChange={setDrawn} />
                  <small className="ui-field-help">계약자 <b>{signerName}</b> (본인인증 실명) · 활동명이나 필명으로는 계약할 수 없어요.</small>
                </div>
                <button type="button" className={`ui-btn is-primary is-block${busy ? ' is-busy' : ''}`} disabled={!all || !drawn || busy} onClick={() => void sign()}>
                  {busy ? '서명하는 중' : '서명하고 계약 맺기'}
                </button>
              </>}
            </div>
          )}
        </section>

        <section className="aq-paper-signoff">
          <p>위 계약의 내용을 확인하고 체결합니다.</p>
          <p className="aq-paper-date">{c.signedAt ? localStamp(c.signedAt) : '서명 전'}</p>
          <div className="aq-paper-parties">
            <div className="aq-paper-signer">
              <span>회사</span>
              <strong>{COMPANY.name}{COMPANY.ceo && <small>대표이사 {COMPANY.ceo}</small>}</strong>
              <span className="aq-paper-sig">{seal ? <img className="is-seal" src={seal} alt={`${COMPANY.name} 직인`} /> : <em>담당자 승인</em>}</span>
            </div>
            <div className="aq-paper-signer">
              <span>작가</span>
              <strong>{c.signerName || signerName || '—'}</strong>
              <span className="aq-paper-sig">{c.signature ? <img src={c.signature} alt={`${c.signerName} 서명`} /> : <em>(서명)</em>}</span>
            </div>
          </div>
          {signed && (
            <div className="aq-paper-record">
              <h3>전자서명 기록</h3>
              <dl>
                <div><dt>서명자</dt><dd>{c.signerName || '—'}</dd></div>
                <div><dt>서명 일시</dt><dd>{c.signedAt ? localStamp(c.signedAt) : '—'}</dd></div>
                {profile.email && <div><dt>서명 계정</dt><dd>{profile.email}</dd></div>}
                <div><dt>서명 방법</dt><dd>AUDENIQ STUDIO 로그인 계정에서 손글씨 전자서명 · 확인 항목 {items.length}개 직접 체크</dd></div>
              </dl>
            </div>
          )}
          <table className="aq-paper-kv aq-pbl-company"><tbody>
            {companyRows().map(([k, v]) => <tr key={k}><th>{k}</th><td>{v}</td></tr>)}
          </tbody></table>
        </section>

        <footer className="aq-paper-foot">
          <div>
            <span>문서 확인 코드 (SHA-256)</span>
            {c.contentHash ? <code>{hashLabel(c.contentHash)}</code> : <em className="aq-paper-pending">서명하면 발급돼요</em>}
          </div>
          <Badge tone={CONTRACT_TONE[c.status]} className="aq-paper-integrity">{CONTRACT_LABEL[c.status]}</Badge>
          <p>이 계약서는 AUDENIQ STUDIO에서 전자문서로 작성되고 전자서명으로 체결돼요. 문서 확인 코드는 계약서 본문·계약 조건·확인 항목·서명으로 계산돼, 내용이 바뀌면 달라져요.</p>
        </footer>
      </article>
    </div>
  );
}
