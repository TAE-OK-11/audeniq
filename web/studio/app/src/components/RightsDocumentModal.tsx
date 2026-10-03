import { useRef, useState } from 'react';
import { Modal } from './Modal';
import { Segmented } from './Segmented';
import { RIGHTS_DOCUMENTS, RIGHTS_SIGNER_ROLES, rightsDocumentBody, type RightsDocumentContext, type RightsDocumentKind, type SigningChannel } from '../lib/rightsDocument';
import { setMockSigningContext, signingApi, signingUrl, type IssuedLink } from '../api/signing';
import { errorMessage } from '../api/errors';
import { currentPath, useNavigate } from '../lib/router';

/**
 * AUDENIQ 전자 문서 도구 — 발매 정보로 권리 서류를 만들고, 권리자 본인에게 서명을 요청한다.
 * 서명은 권리자가 본인확인 후 직접: 링크를 보내거나(LINK), 이 기기를 건네서(IN_PERSON).
 */
export function RightsDocumentModal({ kind, context, onClose, onRequested }: {
  kind: RightsDocumentKind; context: RightsDocumentContext; onClose: () => void;
  /** 서명 요청이 만들어졌을 때 (목록 새로고침용) */
  onRequested?: () => void;
}) {
  const nav = useNavigate();
  const [phase, setPhase] = useState<'write' | 'review' | 'sent'>('write');
  const [rightsHolder, setRightsHolder] = useState('');
  const [signerName, setSignerName] = useState('');
  const [role, setRole] = useState<string>(RIGHTS_SIGNER_ROLES[0]);
  const [source, setSource] = useState(context.source ?? context.tracks.map(t => t.title).join(', '));
  const [scope, setScope] = useState(kind === 'shared' ? '' : '위에 기재한 대상의 디지털 음원 배급·서비스');
  const [period, setPeriod] = useState('서명일부터 해당 발매의 배급 계약 종료일까지');
  const [conditions, setConditions] = useState('별도 대가 없음');
  const [channel, setChannel] = useState<SigningChannel>('LINK');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [link, setLink] = useState<IssuedLink | null>(null);
  const [copied, setCopied] = useState(false);
  // 응답을 놓쳐 다시 눌러도 같은 문서번호 → 서버가 같은 요청으로 링크만 새로 준다
  const documentNo = useRef(crypto.randomUUID());
  const signer = role === RIGHTS_SIGNER_ROLES[0] ? rightsHolder : signerName;
  const body = rightsDocumentBody(kind, context, {
    documentNo: documentNo.current, rightsHolder, signerName: signer, signerRole: role, source, scope, period, conditions,
  });
  const title = `${context.title} · ${RIGHTS_DOCUMENTS[kind].title}`.slice(0, 200);

  const send = async () => {
    if (busy) return;
    setBusy(true); setError('');
    try {
      setMockSigningContext({ releaseTitle: context.title, artist: context.artist });
      const issued = await signingApi.create({
        release_id: context.releaseId, document_no: documentNo.current, document_kind: kind, title, body,
        rights_holder: rightsHolder.trim(), signer_name: signer.trim(), signer_role: role, channel,
      });
      onRequested?.();
      if (channel === 'IN_PERSON') {
        // 권리자에게 기기를 건네 바로 서명 — 끝나면 지금 화면으로 돌아온다
        nav(`/sign/${issued.token}?from=${encodeURIComponent(currentPath())}`);
        return;
      }
      setLink(issued);
      setPhase('sent');
    } catch (e) { setError(errorMessage(e, '서명 요청을 만들지 못했어요. 입력한 내용을 유지했으니 다시 시도해 주세요.')); }
    finally { setBusy(false); }
  };

  const url = link ? signingUrl(link.token) : '';
  const copy = async () => {
    try { await navigator.clipboard.writeText(url); setCopied(true); } catch { setCopied(false); }
  };
  const share = async () => {
    try { await navigator.share({ title: 'AUDENIQ 전자 문서 서명 요청', text: `${title} 서명을 부탁드려요.`, url }); } catch { /* 취소 */ }
  };

  return <Modal modalClass="aq-rights-document-mode" title="AUDENIQ 전자 문서 도구" onClose={onClose} dismissible={!busy && phase !== 'review'} closeDisabled={busy}>
    <p className="eyebrow">{phase === 'write' ? '01 / 03 · 자동 작성' : phase === 'review' ? '02 / 03 · 내용 확인·서명 요청' : '03 / 03 · 링크 보내기'}</p>
    <h3>{RIGHTS_DOCUMENTS[kind].title}</h3>
    {phase === 'write' && <p className="small muted">발매·트랙·배급 범위는 신청서에서 채웠어요. 권리자가 여러 명이면 권리자마다 문서를 따로 만들어 주세요.</p>}

    {phase === 'write' && <form onSubmit={e => {
      e.preventDefault();
      if (![rightsHolder, source, scope, period, conditions].every(value => value.trim()) || (role !== RIGHTS_SIGNER_ROLES[0] && !signerName.trim())) {
        setError('권리자·서명자와 허락 대상·범위·기간·조건을 모두 입력해 주세요.'); return;
      }
      setPhase('review'); setError('');
    }}>
      <div className="field"><label htmlFor="aqDocRightsHolder">{kind === 'shared' ? '위임하는 공동 권리자' : '허락하는 권리자'} 성명·법인명 *</label><input id="aqDocRightsHolder" required maxLength={120} value={rightsHolder} onChange={e => setRightsHolder(e.target.value)} placeholder="원곡 저작권자·실연자·공동 권리자" /></div>
      <div className="field"><label htmlFor="aqRightsSignerRole">서명하는 사람</label><select id="aqRightsSignerRole" value={role} onChange={e => setRole(e.target.value)}>{RIGHTS_SIGNER_ROLES.map(r => <option key={r}>{r}</option>)}</select></div>
      {role !== RIGHTS_SIGNER_ROLES[0] && <div className="field"><label htmlFor="aqRightsSigner">서명자 실명 *</label><input id="aqRightsSigner" maxLength={120} value={signerName} onChange={e => setSignerName(e.target.value)} placeholder="본인확인할 실명" /><p className="help">이 이름으로 본인확인해요. {role === '법인 대표자' ? '대표자 본인이어야 해요.' : '권리자의 위임장을 요청받을 수 있어요.'}</p></div>}
      <div className="field"><label htmlFor="aqDocSource">대상 저작물·원본·참여 내용 *</label><textarea id="aqDocSource" required maxLength={900} rows={3} value={source} onChange={e => setSource(e.target.value)} /><p className="help">원곡, 샘플 출처·사용 구간, 참여 트랙 등 허락할 대상을 적어 주세요.</p></div>
      <div className="field"><label htmlFor="aqDocScope">{kind === 'shared' ? '위임하는 지분·범위' : '허락하는 권리·지분·사용 범위'} *</label><textarea id="aqDocScope" required maxLength={700} rows={2} value={scope} onChange={e => setScope(e.target.value)} placeholder="예: 작곡 권리 지분 50%의 디지털 배급 이용 허락" /></div>
      <div className="field"><label htmlFor="aqDocPeriod">기간 *</label><input id="aqDocPeriod" required maxLength={200} value={period} onChange={e => setPeriod(e.target.value)} /></div>
      <div className="field"><label htmlFor="aqDocConditions">대가와 조건 *</label><textarea id="aqDocConditions" required maxLength={700} rows={2} value={conditions} onChange={e => setConditions(e.target.value)} placeholder="예: 순수익의 30%를 매 분기 정산" /></div>
      <button className="button studio-submit-wide" type="submit">자동 작성된 문서 확인하기</button>
    </form>}

    {phase === 'review' && <>
      <div className="aq-document-snapshot">{body}</div>
      <p className="aq-option-note">문서는 {signer.trim()}님이 본인확인(휴대폰·간편인증)을 거친 뒤 직접 읽고 서명해요. 서명하면 내용을 바꿀 수 없어요.</p>
      <span className="aq-method-label" id="aqSignChannelLabel">서명 받는 방법</span>
      <Segmented className="aq-method" labelledBy="aqSignChannelLabel" value={channel} onChange={setChannel}
        options={[{ value: 'LINK', label: '서명 링크 보내기' }, { value: 'IN_PERSON', label: '이 기기에서 바로' }] as const} />
      <p className="small muted">{channel === 'LINK'
        ? '링크를 카카오톡·문자·메일로 보내면, 권리자가 7일 안에 자기 휴대폰에서 서명해요.'
        : '이 기기를 권리자에게 건네 주세요. 1시간 안에 권리자 본인이 확인하고 서명한 뒤 돌려주면 돼요.'}</p>
      <div className="doc-actions"><button type="button" className="button secondary" disabled={busy} onClick={() => setPhase('write')}>내용 수정</button>
        <button type="button" className="button" disabled={busy} onClick={() => void send()}>{busy ? '요청 만드는 중…' : channel === 'LINK' ? '서명 링크 만들기' : '권리자에게 기기 건네기'}</button></div>
    </>}

    {phase === 'sent' && link && <>
      <p>{signer.trim()}님께 아래 링크를 보내 주세요. 링크를 연 사람은 본인확인을 해야 서명할 수 있어요.</p>
      <div className="aq-sign-link"><code>{url}</code></div>
      <div className="doc-actions">
        <button type="button" className="button secondary" onClick={() => void copy()}>{copied ? '복사했어요' : '링크 복사'}</button>
        {typeof navigator.share === 'function' && <button type="button" className="button secondary" onClick={() => void share()}>공유하기</button>}
        <button type="button" className="button" onClick={onClose}>완료</button>
      </div>
      <p className="small muted">링크는 {new Date(link.expires_at).toLocaleString('ko-KR', { dateStyle: 'long', timeStyle: 'short' })}까지 쓸 수 있고, 지금 화면을 닫으면 다시 볼 수 없어요. 잃어버리면 권리·보완 서류의 서명 요청에서 새 링크를 만들 수 있어요.</p>
    </>}
    {error && <p role="alert" className="aq-rights-error">{error}</p>}
  </Modal>;
}
