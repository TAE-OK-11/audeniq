import { useEffect, useRef, useState } from 'react';
import { Modal } from './Modal';
import { SignaturePad, type SignaturePadHandle } from './SignaturePad';
import { compactSignature, sha256Hex } from '../lib/application';
import { RIGHTS_DOCUMENTS, RIGHTS_FORM, RIGHTS_SIGNER_ROLES, rightsDocumentBody, rightsHashInput, type RightsDocumentContext, type RightsDocumentKind } from '../lib/rightsDocument';
import { MOCK } from '../lib/mode';
import { stampNow } from '../lib/date';
import { addDoc, type DocRecord } from '../store/docs';
import { createElectronicDocument, electronicRightsAvailable, fetchDocuments } from '../api/portal';
import { setDocs } from '../store/docs';
import { errorMessage } from '../api/errors';

export function RightsDocumentModal({ kind, context, onClose, onComplete }: {
  kind: RightsDocumentKind; context: RightsDocumentContext; onClose: () => void; onComplete: (doc: DocRecord) => void;
}) {
  const [phase, setPhase] = useState<'write' | 'sign'>('write');
  const [rightsHolder, setRightsHolder] = useState('');
  const [source, setSource] = useState(context.source ?? context.tracks.map(t => t.title).join(', '));
  const [scope, setScope] = useState(kind === 'shared' ? '' : '위에 기재한 대상의 디지털 음원 배급·서비스');
  const [period, setPeriod] = useState('서명일부터 해당 발매의 배급 계약 종료일까지');
  const [conditions, setConditions] = useState('별도 조건 없음');
  const [signerName, setSignerName] = useState('');
  const [role, setRole] = useState<string>(RIGHTS_SIGNER_ROLES[0]);
  const [consent, setConsent] = useState(false);
  const [signed, setSigned] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [available, setAvailable] = useState<boolean | null>(MOCK ? true : null);
  useEffect(() => {
    if (MOCK) return;
    let active = true;
    electronicRightsAvailable().then(ready => { if (active) setAvailable(ready); });
    return () => { active = false; };
  }, []);
  const pad = useRef<SignaturePadHandle>(null);
  // Retry after a lost response uses the same receipt number and signature.
  const receipt = useRef<{ documentNo: string; signature: string } | null>(null);
  const body = rightsDocumentBody(kind, context, { rightsHolder, source, scope, period, conditions });
  const title = `${context.title} · ${RIGHTS_DOCUMENTS[kind].title}`.slice(0, 200);

  const complete = async () => {
    if (busy || available !== true) return;
    if (!signerName.trim() || !signed || pad.current?.isEmpty() || !consent) {
      setError('서명자 성명, 직접 서명, 내용 및 서명 권한 동의를 모두 확인해 주세요.'); return;
    }
    setBusy(true); setError('');
    try {
      if (!receipt.current) receipt.current = { documentNo: crypto.randomUUID(), signature: await compactSignature(pad.current?.toDataURL() ?? '') };
      const electronic = { document_no: receipt.current.documentNo, form: RIGHTS_FORM, document_kind: kind,
        rights_holder: rightsHolder.trim(), signer_name: signerName.trim(), signer_role: role,
        signature: receipt.current.signature, consent };
      const at = stampNow();
      let doc: DocRecord;
      if (MOCK) {
        const { signature: _signature, consent: _consent, ...record } = electronic;
        doc = { id: receipt.current.documentNo, kind: 'rights', releaseId: context.releaseId, releaseTitle: context.title,
          title, content: body, version: '1.0', created: at, fileName: '', checked: true, checkedAt: at,
          consentHistory: [{ time: at, action: '내용 확인', version: '1.0' }],
          reviewHistory: [{ status: '전자 문서 서명 완료', time: at, detail: '권리자 서명 문서 완성 · AUDENIQ 검토 대기' }],
          reviewStatus: 'review', reviewNote: '', signerName: electronic.signer_name,
          localSignatureData: electronic.signature, localSignatureAt: at,
          electronic: { ...record, content_hash: await sha256Hex(rightsHashInput(title, body, electronic)) } };
        addDoc(doc);
      } else {
        const id = await createElectronicDocument(context.releaseId, title, body, electronic);
        const docs = await fetchDocuments();
        setDocs(docs);
        const saved = docs.find(d => d.id === id);
        if (!saved) throw new Error('완성된 문서를 다시 불러오지 못했어요. 다시 눌러 확인해 주세요.');
        doc = saved;
      }
      onComplete(doc);
    } catch (e) { setError(errorMessage(e, '전자 문서를 저장하지 못했어요. 입력한 내용을 유지했으니 다시 시도해 주세요.')); }
    finally { setBusy(false); }
  };

  return <Modal modalClass="aq-rights-document-mode" title="AUDENIQ 전자 문서 도구" onClose={onClose} dismissible={!busy} closeDisabled={busy}>
    <p className="eyebrow">{phase === 'write' ? '01 / 02 · 자동 작성' : '02 / 02 · 내용 확인 및 서명'}</p>
    <h3>{RIGHTS_DOCUMENTS[kind].title}</h3>
    <p className="small muted">발매·트랙·배급 범위는 신청서에서 채웠어요. 권리자가 여러 명이면 각 권리자별로 문서를 작성해 주세요.</p>
    {available === null && <p role="status" className="small muted">전자 문서 접수 상태를 확인하고 있어요.</p>}
    {available === false && <p role="status" className="notice">전자 문서 접수는 준비 중이에요. 현재는 보유한 허락서·동의서를 첨부해 주세요.</p>}
    {phase === 'write' ? <form onSubmit={e => {
      e.preventDefault();
      if (![rightsHolder, source, scope, period, conditions].every(value => value.trim())) {
        setError('권리자와 허락 대상·범위·기간·조건을 모두 입력해 주세요.'); return;
      }
      setSignerName(rightsHolder.trim()); setPhase('sign'); setError('');
    }}>
      <div className="field"><label htmlFor="aqDocRightsHolder">허락하는 권리자 성명·법인명 *</label><input id="aqDocRightsHolder" required maxLength={120} value={rightsHolder} onChange={e => setRightsHolder(e.target.value)} placeholder="원곡 저작권자·실연자·공동 권리자" /></div>
      <div className="field"><label htmlFor="aqDocSource">대상 저작물·원본·참여 내용 *</label><textarea id="aqDocSource" required maxLength={900} rows={3} value={source} onChange={e => setSource(e.target.value)} /><p className="help">원곡, 샘플 출처·사용 구간, 참여 트랙 등 허락할 대상을 확인해 주세요.</p></div>
      <div className="field"><label htmlFor="aqDocScope">허락하는 권리·지분·사용 범위 *</label><textarea id="aqDocScope" required maxLength={700} rows={2} value={scope} onChange={e => setScope(e.target.value)} placeholder="예: 작곡 권리 지분 50%의 디지털 배급 이용 허락" /></div>
      <div className="field"><label htmlFor="aqDocPeriod">허락 기간 *</label><input id="aqDocPeriod" required maxLength={200} value={period} onChange={e => setPeriod(e.target.value)} /></div>
      <div className="field"><label htmlFor="aqDocConditions">추가 조건·대가 *</label><textarea id="aqDocConditions" required maxLength={700} rows={2} value={conditions} onChange={e => setConditions(e.target.value)} /></div>
      <button className="button studio-submit-wide" type="submit" disabled={available !== true}>자동 작성된 문서 확인하기</button>
    </form> : <>
      <div className="aq-document-snapshot">{body}</div>
      <p className="aq-option-note">권리자 또는 위임받은 대리인이 내용을 확인하고 직접 서명해 주세요. 현재 로그인 계정의 제출 기록과 함께 보관돼요.</p>
      <div className="field"><label htmlFor="aqRightsSigner">직접 서명하는 사람의 성명 *</label><input id="aqRightsSigner" maxLength={120} value={signerName} disabled={busy} onChange={e => { setSignerName(e.target.value); receipt.current = null; }} /></div>
      <div className="field"><label htmlFor="aqRightsSignerRole">서명자 구분</label><select id="aqRightsSignerRole" value={role} disabled={busy} onChange={e => { setRole(e.target.value); receipt.current = null; }}>{RIGHTS_SIGNER_ROLES.map(r => <option key={r}>{r}</option>)}</select></div>
      <div className={busy ? 'aq-rights-pad is-busy' : 'aq-rights-pad'} inert={busy}><SignaturePad ref={pad} id="aqRightsPad" label="권리자 직접 서명" onChange={value => { setSigned(value); receipt.current = null; }} /></div>
      <button type="button" className="link-btn" disabled={busy} onClick={() => { pad.current?.clear(); receipt.current = null; }}>서명 다시 그리기</button>
      <label className="check-line"><input id="aqRightsConsent" type="checkbox" checked={consent} disabled={busy} onChange={e => setConsent(e.target.checked)} /><span>문서 내용을 확인했고, 기재한 권리자 또는 위임받은 대리인으로서 직접 서명합니다. 내용과 서명 기록의 보관에 동의합니다.</span></label>
      <div className="doc-actions"><button type="button" className="button secondary" disabled={busy} onClick={() => { setPhase('write'); setConsent(false); setSigned(false); receipt.current = null; }}>내용 수정</button><button type="button" className="button" disabled={busy} onClick={() => void complete()}>{busy ? '문서 저장 중…' : '서명하고 문서 완성'}</button></div>
    </>}
    {error && <p role="alert" className="aq-rights-error">{error}</p>}
  </Modal>;
}
