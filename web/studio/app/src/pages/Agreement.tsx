// 배급 계약서 (/contracts/:id) — 서버가 만든 계약서 본문을 신청서와 같은 정식 서류로 보여 주고 인쇄·PDF로 저장한다.
// 서명도 이 화면에서 — 저작권 관리 계약서와 같은 틀: 위 '서명 전' 안내 → 본문 → '계약 확인'(확인 항목 체크 · 서명자 · 손 서명) → 서명란.
// 따로 뜨는 서명 창을 거치지 않는다 (?sign=1로 들어오면 '계약 확인'으로 바로 내려간다)
import { useEffect, useMemo, useRef, useState } from 'react';
import { printWithStamp } from '../lib/printStamp';
import { Link, useNavigate, useParams, useSearchParams } from '../lib/router';
import { useDocs, setDocs, docState, docBadgeTone, updateDoc } from '../store/docs';
import { Badge } from '../ui';
import { checkDocument, fetchDocuments, signDocument } from '../api/portal';
import { MOCK } from '../lib/mode';
import { errorMessage } from '../api/errors';
import { localStamp, stripSampleSuffix } from '../lib/format';
import { parseStamp, stampNow } from '../lib/date';
import { compactSignature, hashLabel } from '../lib/application';
import { SkeletonBlock } from '../components/Skeleton';
import { CheckIcon } from '../components/Check';
import { Glyph } from '../components/Glyph';
import { parseAgreement, type Block } from '../lib/agreementDoc';
import { COMPANY, companyRows, companySeal } from '../lib/company';
import { useProfile } from '../store/profile';
import { useAccount } from '../store/account';
import { uid } from '../lib/store';
import { pushNotice } from '../store/support';
import { refreshDocs } from '../store/portalSync';
import { useToast } from '../components/Toast';
import { SignaturePad, type SignaturePadHandle } from '../components/SignaturePad';
import '../styles/split/Upload.css';

function longDate(stamp: string): string {
  const d = parseStamp(stamp);
  return d ? `${d.getFullYear()}년 ${d.getMonth() + 1}월 ${d.getDate()}일` : stamp;
}

export function Agreement() {
  const { id = '' } = useParams();
  const nav = useNavigate();
  const doc = useDocs().find(d => d.id === id && d.kind === 'agreements');
  const profile = useProfile();
  const account = useAccount();
  const toast = useToast();
  const [params, setParams] = useSearchParams();
  const signRef = useRef<HTMLElement>(null);
  const pad = useRef<SignaturePadHandle>(null);
  const [ticked, setTicked] = useState<string[]>([]);
  const [drawn, setDrawn] = useState(false);
  const [busy, setBusy] = useState(false);
  // 서명자 — 본인인증 실명이 있으면 그것, 없으면 활동명 (레이블·법인은 고쳐 적는다)
  const [signer, setSigner] = useState('');
  const [loading, setLoading] = useState(!MOCK);
  const [error, setError] = useState('');
  useEffect(() => {
    if (MOCK) return;
    let live = true;
    fetchDocuments().then(list => { if (live) setDocs(list); })
      .catch(e => { if (live) setError(errorMessage(e)); })
      .finally(() => { if (live) setLoading(false); });
    return () => { live = false; };
  }, [id]);
  const parsed = useMemo(() => parseAgreement(doc?.content ?? ''), [doc?.content]);
  useEffect(() => { setSigner(s => s || doc?.signerName || (account.verified ? account.realName.trim() : '') || profile.name.trim()); }, [doc?.id]); // eslint-disable-line react-hooks/exhaustive-deps
  // 계약서 목록·발매 상세의 '확인하고 서명'에서 왔으면 '계약 확인'으로 바로
  const wantSign = params.get('sign') === '1';
  useEffect(() => {
    if (!wantSign || !doc) return;
    const t = window.setTimeout(() => signRef.current?.scrollIntoView({ behavior: 'smooth', block: 'start' }), 150);
    setParams({}, { replace: true });
    return () => window.clearTimeout(t);
  }, [wantSign, doc?.id]); // eslint-disable-line react-hooks/exhaustive-deps

  if (loading && !doc) return <div className="view aq-doc-view"><SkeletonBlock height={640} /></div>;
  if (error || !doc) {
    return (
      <div className="ui-empty">
        <h2>계약서를 확인할 수 없어요.</h2>
        <p>{error || '이 계정에서 볼 수 있는 계약서가 없어요.'}</p>
        <button type="button" className="ui-btn is-soft" onClick={() => nav('/contracts')}>계약서 목록</button>
      </div>
    );
  }

  const signed = !!doc.localSignatureAt;
  const canSign = !signed && doc.reviewStatus === 'approved';
  // 계약서 별첨의 확인 항목 — 없는 계약서(예전 양식)는 '내용을 읽었다'는 한 줄만 체크한다 (서버에는 보내지 않는다)
  const termBoxes = doc.agreementTerms?.confirmations ?? [];
  const boxes = termBoxes.length ? termBoxes : [{ id: '__read', text: '계약서 내용을 모두 읽고 확인했어요.', required: true }];
  const all = boxes.every(b => ticked.includes(b.id));
  const toggle = (k: string) => setTicked(t => (t.includes(k) ? t.filter(x => x !== k) : [...t, k]));
  const sign = async () => {
    if (busy || !all || !drawn || !signer.trim()) return;
    setBusy(true);
    try {
      const data = pad.current?.toDataURL() ?? '';
      if (MOCK) {
        const at = stampNow();
        updateDoc(doc.id, {
          ...(termBoxes.length ? { confirmations: { items: termBoxes.map(b => ({ ...b, checked: ticked.includes(b.id) })), content_hash: '' } } : {}),
          signerName: signer.trim(), localSignatureData: data, localSignatureAt: at,
          reviewHistory: [...doc.reviewHistory, { status: '계약 체결', time: at, detail: '확인 항목을 체크하고 서명했어요' }],
        });
        pushNotice({
          id: uid('n'), kind: '서류', time: at, link: `/contracts/${doc.id}`,
          title: `${stripSampleSuffix(doc.releaseTitle || doc.title)} 계약서에 서명했어요.`,
          detail: '계약서 사본은 계약서 메뉴에서 언제든 볼 수 있어요.',
        });
      } else {
        // 확인 기록은 여러 번 해도 같고 서버의 지금 문서 버전을 돌려준다 — 화면을 연 뒤 조건이 바뀌어도 옛 버전으로 거절되지 않게
        const rv = await checkDocument(doc.id);
        const compact = await compactSignature(data);
        await signDocument(doc.id, signer.trim(), compact || data, rv, termBoxes.filter(b => ticked.includes(b.id)).map(b => b.id));
        await refreshDocs();
      }
      toast('계약을 맺었어요. 발매일에 맞춰 배급을 시작해요.', 'success');
      window.scrollTo({ top: 0, behavior: 'smooth' });
    } catch (e) { toast(errorMessage(e, '서명하지 못했어요. 다시 시도해 주세요.'), 'error'); }
    finally { setBusy(false); }
  };
  const terms = doc.agreementTerms;
  const checked = (text: string) => doc.confirmations?.items.find(i => i.text === text)?.checked;
  const docNo = `AUD-DIST-${doc.id.replace(/[^0-9a-z]/gi, '').slice(0, 8).toUpperCase()}`;
  const meta: [string, string][] = [
    ['계약서 번호', docNo],
    ...parsed.meta.filter(([k]) => k !== '적용 약관'),
    ['체결일', signed ? localStamp(doc.localSignatureAt) : '서명 전'],
  ];
  const termsRef = parsed.meta.find(([k]) => k === '적용 약관')?.[1];
  const seal = companySeal();
  // 서명 전에도 서명란에 이용자 이름을 — 계약 당사자의 '서명자' 칸에서 (괄호 설명은 뗀다)
  const partyName = parsed.articles.flatMap(a => a.blocks).flatMap(b => (b.kind === 'kv' ? b.rows : []))
    .find(([k]) => k === '서명자')?.[1].replace(/\s*\(.*\)\s*$/, '');

  const renderBlocks = (blocks: Block[]) => blocks.map((b, i) => {
    switch (b.kind) {
      case 'kv': return (
        <table key={i} className="aq-paper-kv"><tbody>
          {b.rows.flatMap(([k, v]) => (k === '회사' ? [[k, v] as [string, string], ...companyRows()] : [[k, v] as [string, string]]))
            .map(([k, v]) => <tr key={k}><th>{k}</th><td>{v}</td></tr>)}
        </tbody></table>
      );
      case 'list': return <ol key={i} className="aq-paper-tracks">{b.items.map(t => <li key={t}>{t}</li>)}</ol>;
      case 'check': return (
        <ol key={i} className="aq-paper-agree">
          {b.items.map(it => {
            const on = signed ? !!checked(it.text) : false;
            return (
              <li key={it.text} className={on ? 'is-on' : ''}>
                <span className="aq-paper-check" role="img" aria-label={on ? '확인함' : '확인 전'}>{on ? <CheckIcon size={12} /> : null}</span>
                <span><b className="aq-paper-req">{it.required ? '필수' : '해당 시'}</b> {it.text}</span>
              </li>
            );
          })}
        </ol>
      );
      default: return <p key={i} className="aq-paper-p">{b.text}</p>;
    }
  });

  return (
    <div id="view-agreement" className="view aq-doc-view">
      {/* 뒤로 가기·인쇄 줄이 맨 위(발매 상세·신청서와 같은 자리), 그 아래 서명 안내 — 파란 버튼은 '확인하고 서명' 하나 */}
      <div className="aq-doc-toolbar">
        <button type="button" className="ui-btn is-white is-sm aq-back-link" onClick={() => nav('/contracts')}><Glyph name="arrow-left" size={15} className="aq-glyph-lead" />계약서</button>
        <div className="row-actions">
          {doc.releaseId && <button type="button" className="ui-btn is-soft" onClick={() => nav(`/releases/${doc.releaseId}/application`)}>신청서 보기</button>}
          <button type="button" className="ui-btn is-soft" onClick={() => void printWithStamp({ kind: 'DOCUMENT', ref: id, root: document.querySelector('.aq-paper') })}>인쇄 · PDF 저장</button>
        </div>
      </div>

      {!signed && (
        <div className="aq-doc-done is-pending" role="status">
          <span className="aq-doc-done-mark" aria-hidden="true"><Glyph name="pencil" size={18} /></span>
          <div className="min-0">
            <strong>{doc.reviewStatus === 'approved' ? '서명 전 계약서예요.' : '담당자 검토 중인 계약서예요.'}</strong>
            <span>{doc.reviewStatus === 'approved' ? '내용을 읽고 권리 확인 항목에 체크한 뒤 서명하면 계약이 체결돼요.' : '검토가 끝나면 계약 조건이 확정되고 서명할 수 있어요.'}</span>
          </div>
          {canSign && <button type="button" className="ui-btn is-primary" onClick={() => signRef.current?.scrollIntoView({ behavior: 'smooth', block: 'start' })}>확인하고 서명</button>}
        </div>
      )}


      <article className="aq-paper" aria-label="음원 배급 계약서">
        <header className="aq-paper-head">
          <div className="aq-paper-brand">
            <img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" />
            <dl className="aq-paper-meta">
              {meta.map(([k, v]) => <div key={k}><dt>{k}</dt><dd>{v}</dd></div>)}
            </dl>
          </div>
          <h1>음원 배급 계약서</h1>
          <p className="aq-paper-en">Digital Music Distribution Agreement</p>
        </header>

        {parsed.intro.map(t => <p key={t} className="aq-paper-lead">{t}</p>)}

        {parsed.articles.map(a => (
          <section key={a.no} className="aq-paper-sec">
            <h2><span>{a.no}</span>{a.title}</h2>
            {renderBlocks(a.blocks)}
          </section>
        ))}

        {parsed.annexes.map(a => (
          <section key={a.no} className="aq-paper-sec aq-paper-annex">
            <h2><em>{a.no}</em>{a.title}</h2>
            {renderBlocks(a.blocks)}
          </section>
        ))}

        {/* 계약 확인 — 저작권 관리 계약서와 같은 칸: 확인 항목 모두 체크 → 서명자 → 손 서명 → 서명하고 계약 맺기 */}
        {canSign && (
          <section className="aq-paper-sec aq-doc-sign-sec" ref={signRef}>
            <h2>계약 확인</h2>
            <div className="aq-doc-sign">
              {(
                <div className="aq-agree-box">
                  <label className="ui-check is-plain aq-agree-all-line">
                    <input type="checkbox" checked={all} onChange={e => setTicked(e.target.checked ? boxes.map(b => b.id) : [])} />
                    <span><strong>아래 내용을 모두 확인했어요</strong></span>
                  </label>
                  {boxes.map(b => (
                    <label key={b.id} className="ui-check is-plain">
                      <input type="checkbox" checked={ticked.includes(b.id)} onChange={() => toggle(b.id)} />
                      <span><b className="aq-paper-req">{b.required ? '필수' : '해당 시'}</b> {b.text}</span>
                    </label>
                  ))}
                </div>
              )}
              <div className="ui-field">
                <label htmlFor="distSigner">서명자 <span className="required">*</span></label>
                <input id="distSigner" className="ui-input" maxLength={80} value={signer} onChange={e => setSigner(e.target.value)} placeholder="실명 또는 법인명" />
              </div>
              <div className="ui-field">
                <label htmlFor="distContractPad">서명 <span className="required">*</span></label>
                <SignaturePad ref={pad} id="distContractPad" label="이용자 서명 입력" height={170} onChange={setDrawn} />
              </div>
              <button type="button" className={`ui-btn is-primary is-block${busy ? ' is-busy' : ''}`} disabled={!all || !drawn || !signer.trim() || busy} onClick={() => void sign()}>
                {busy ? '서명하는 중' : '서명하고 계약 맺기'}
              </button>
            </div>
          </section>
        )}

        <section className="aq-paper-signoff">
          <p>위 계약의 내용을 확인하고 체결합니다.</p>
          <p className="aq-paper-date">{signed ? longDate(doc.localSignatureAt) : '서명 전'}</p>
          <div className="aq-paper-parties">
            <div className="aq-paper-signer">
              <span>회사</span>
              <strong>{COMPANY.name}{COMPANY.ceo && <small>대표이사 {COMPANY.ceo}</small>}</strong>
              <span className="aq-paper-sig">
                {doc.approvedAt && seal ? <img className="is-seal" src={seal} alt={`${COMPANY.name} 직인`} /> : <em>{doc.approvedAt ? `담당자 승인 · ${longDate(doc.approvedAt)}` : '담당자 승인 전'}</em>}
              </span>
            </div>
            <div className="aq-paper-signer">
              <span>이용자</span>
              <strong>{doc.signerName || partyName || '—'}</strong>
              <span className="aq-paper-sig">
                {doc.localSignatureData ? <img src={doc.localSignatureData} alt={`${doc.signerName} 서명`} /> : <em>(서명)</em>}
              </span>
            </div>
          </div>
          {signed && (
            <div className="aq-paper-record">
              <h3>전자서명 기록</h3>
              <dl>
                <div><dt>서명자</dt><dd>{doc.signerName || partyName || '—'}</dd></div>
                <div><dt>서명 일시</dt><dd>{localStamp(doc.localSignatureAt)}</dd></div>
                {profile.email && <div><dt>서명 계정</dt><dd>{profile.email}</dd></div>}
                <div><dt>서명 방법</dt><dd>AUDENIQ STUDIO 로그인 계정에서 손글씨 전자서명{doc.confirmations?.items.length ? ` · 확인 항목 ${doc.confirmations.items.filter(i => i.checked).length}개 직접 체크` : ''}</dd></div>
                <div><dt>회사 승인</dt><dd>{doc.approvedAt ? localStamp(doc.approvedAt) : '승인 전'}</dd></div>
              </dl>
            </div>
          )}
        </section>

        <footer className="aq-paper-foot">
          <div>
            <span>문서 확인 코드 (SHA-256)</span>
            {doc.confirmations?.content_hash ? <code>{hashLabel(doc.confirmations.content_hash)}</code> : <em className="aq-paper-pending">서명하면 발급돼요</em>}
          </div>
          <Badge tone={docBadgeTone(doc)} className="aq-paper-integrity">{docState(doc)}</Badge>
          <p>
            이 계약서는 AUDENIQ STUDIO에서 전자문서로 작성되고 전자서명으로 체결돼요.
            {terms && <> 배급수수료 회사 {terms.fee_bps / 100}% · 이용자 {terms.user_bps / 100}%, {terms.exclusivity === 'EXCLUSIVE' ? '독점' : '비독점'} 배급. </>}
            문서 확인 코드는 계약서 본문·계약 조건·확인 항목·서명으로 계산돼, 내용이 바뀌면 달라져요.
            {termsRef && <> 이 계약에서 정하지 않은 사항은 <Link to="/terms">{termsRef}</Link>을 따라요.</>}
          </p>
        </footer>
      </article>
    </div>
  );
}
