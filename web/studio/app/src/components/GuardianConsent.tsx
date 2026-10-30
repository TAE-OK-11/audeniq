// 미성년 아티스트 발매의 법정대리인 동의 — 서명과 가족관계증명서가 모두 준비되면 신청할 수 있어요.
// 서명은 법정대리인이 직접 본인확인 후 해요 (이 기기에서, 또는 보낸 링크에서). 아티스트가 대신하지 않아요.
import { useEffect, useState } from 'react';
import { Modal } from './Modal';
import { CopyButton } from './CopyButton';
import { CheckIcon } from './Check';
import { errorMessage } from '../api/errors';
import { guardianApi, type GuardianStatus } from '../api/guardian';
import { signingUrl } from '../api/signing';

const POLL_MS = 4000;

function signStep(s: GuardianStatus | null): { done: boolean; text: string } {
  const r = s?.requests[0];
  if (!r) return { done: false, text: '아직 서명 전이에요.' };
  switch (r.status) {
    case 'SIGNED': return { done: true, text: '서명을 마쳤어요.' };
    case 'VERIFIED': return { done: false, text: '본인확인을 마쳤어요. 서명만 남았어요.' };
    case 'DECLINED': return { done: false, text: '법정대리인이 서명하지 않았어요. 내용을 확인하고 다시 요청해 주세요.' };
    case 'EXPIRED': return { done: false, text: '링크 기한이 지났어요. 다시 열어 주세요.' };
    case 'CANCELLED': return { done: false, text: '요청을 취소했어요. 다시 열어 주세요.' };
    default: return { done: false, text: '법정대리인의 본인확인과 서명을 기다리고 있어요.' };
  }
}

function documentStep(s: GuardianStatus | null): { done: boolean; text: string } {
  const d = s?.relation_document;
  if (!d || !d.has_file) return { done: false, text: '아직 올리기 전이에요.' };
  if (d.status === 'APPROVED') return { done: true, text: '확인을 마쳤어요.' };
  if (d.status === 'NEEDS') return { done: true, text: '보완 요청이 있어요. 권리·보완 서류에서 확인해 주세요.' };
  return { done: true, text: '올렸어요. 심사 때 관계를 확인해요.' };
}

export function GuardianConsentPanel({ org, releaseId, onClose, onReady }: {
  org: string;
  releaseId: string;
  onClose: () => void;
  /** 서명과 증명서가 모두 준비됐을 때 */
  onReady?: () => void;
}) {
  const [status, setStatus] = useState<GuardianStatus | null>(null);
  const [link, setLink] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    let live = true;
    // 서명과 증명서가 모두 준비되면 더 바뀔 것이 없다 — 그 뒤로는 4초마다 묻지 않는다
    let ready = false;
    const load = () => guardianApi.status(org, releaseId)
      .then(s => { ready = s.ready; if (live) setStatus(s); })
      .catch(e => { if (live) setError(errorMessage(e, '동의 상태를 불러오지 못했어요.')); });
    void load();
    // 다른 탭·앱에 가 있는 동안은 묻지 않고, 돌아오면 바로 한 번 확인한다
    const t = window.setInterval(() => { if (!ready && document.visibilityState === 'visible') void load(); }, POLL_MS);
    const onVisible = () => { if (!ready && document.visibilityState === 'visible') void load(); };
    document.addEventListener('visibilitychange', onVisible);
    return () => { live = false; window.clearInterval(t); document.removeEventListener('visibilitychange', onVisible); };
  }, [org, releaseId]);

  const guardian = status?.guardian;
  const named = !!guardian?.name && !!guardian.relation;

  async function open(channel: 'IN_PERSON' | 'LINK') {
    if (!guardian || busy) return;
    setBusy(true);
    setError('');
    try {
      const issued = await guardianApi.open(org, releaseId, { guardian_name: guardian.name, relation: guardian.relation, channel });
      const url = signingUrl(issued.token);
      if (channel === 'IN_PERSON') window.open(url, '_blank', 'noopener');
      else setLink(url);
      setStatus(await guardianApi.status(org, releaseId));
    } catch (e) {
      setError(errorMessage(e, '서명 링크를 만들지 못했어요.'));
    } finally {
      setBusy(false);
    }
  }

  const sign = signStep(status);
  const doc = documentStep(status);
  return (
    <Modal title="법정대리인 동의" onClose={onClose} modalClass="aq-guardian-mode">
      <div className="guardian-modal-body">
        <p className="guardian-consent-text">
          미성년 아티스트의 발매는 법정대리인{named ? <strong> {guardian?.name}({guardian?.relation})</strong> : ''}의 동의가 있어야 신청할 수 있어요.
          법정대리인이 직접 본인확인을 하고 서명해요.
        </p>
        {!named && status && (
          <p className="aq-guardian-missing">발매 초안의 특별한 경우 → 미성년 아티스트에서 법정대리인 성명과 관계를 먼저 입력해 주세요.</p>
        )}
        <ol className="aq-guardian-steps">
          <li className={sign.done ? 'is-done' : ''}>
            <span className="aq-guardian-no" aria-hidden="true">{sign.done ? <CheckIcon size={13} /> : 1}</span>
            <div className="min-0">
              <strong className="aq-guardian-label">법정대리인 본인확인과 서명</strong>
              <p className="ui-field-help" aria-live="polite">{sign.text}</p>
              {!sign.done && named && (
                <div className="aq-row-actions">
                  <button type="button" className="ui-btn is-primary is-sm" disabled={busy} onClick={() => void open('IN_PERSON')}>이 기기에서 서명하기</button>
                  <button type="button" className="ui-btn is-soft is-sm" disabled={busy} onClick={() => void open('LINK')}>법정대리인에게 링크 보내기</button>
                </div>
              )}
              {link && !sign.done && (
                <p className="aq-cert-picked">
                  <span className="min-0">{link}</span> <CopyButton text={link} label="링크 복사" />
                  <small>7일 동안 열 수 있어요. 법정대리인 본인에게만 보내 주세요.</small>
                </p>
              )}
            </div>
          </li>
          <li className={doc.done ? 'is-done' : ''}>
            <span className="aq-guardian-no" aria-hidden="true">{doc.done ? <CheckIcon size={13} /> : 2}</span>
            <div className="min-0">
              <strong className="aq-guardian-label">가족관계증명서</strong>
              <p className="ui-field-help" aria-live="polite">{doc.text}</p>
              {status?.relation_document && !status.relation_document.has_file && (
                <a className="ui-btn is-soft is-sm" href={`/rights?doc=${encodeURIComponent(status.relation_document.id)}`} target="_blank" rel="noopener">권리·보완 서류에서 올리기</a>
              )}
              {!status?.relation_document && <p className="ui-field-help">서명을 요청하면 올릴 칸이 생겨요.</p>}
            </div>
          </li>
        </ol>
        {error && <p className="aq-guardian-missing" role="alert">{error}</p>}
      </div>
      <div className="aq-modal-foot aq-guardian-foot aq-sticky-foot">
        <p className="aq-guardian-missing" aria-live="polite">{status && !status.ready ? '서명과 가족관계증명서가 모두 준비되면 신청할 수 있어요.' : ''}</p>
        <button type="button" className="ui-btn is-soft" onClick={onClose}>닫기</button>
        <button type="button" className="ui-btn is-primary" disabled={!status?.ready} onClick={() => onReady?.()}>동의 확인 완료</button>
      </div>
    </Modal>
  );
}
