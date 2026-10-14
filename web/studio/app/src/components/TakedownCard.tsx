// 발매 중단 요청 — 이미 플랫폼에 보낸 발매를 내리고 싶을 때. 담당자가 확인한 뒤 플랫폼마다 중단을 요청하고,
// 플랫폼별로 내려갔는지 보여 준다. 아직 아무 데도 보내지 않은 신청은 '신청 취소'로 처리한다.
import { useCallback, useEffect, useState } from 'react';
import { api } from '../api/client';
import { errorMessage } from '../api/errors';
import type { TakedownState } from '../api/types';
import { dspLabel } from '../lib/catalog';
import { useConfirm } from './Confirm';
import { Modal, useModalClose } from './Modal';
import { useToast } from './Toast';

const PLATFORM_STATE: Record<string, string> = {
  LIVE: '서비스 중',
  INGESTING: '등록 중',
  UNKNOWN: '전송 완료',
  TAKEDOWN_REQUESTED: '중단 요청함',
  TAKEN_DOWN: '내려감',
};

const REQUEST_STATE: Record<string, string> = {
  PENDING: '담당자가 확인하고 있어요.',
  APPROVED: '플랫폼에 중단을 요청했어요. 플랫폼마다 내려가는 데 며칠 걸릴 수 있어요.',
  DECLINED: '요청이 반려됐어요.',
  CANCELLED: '요청을 취소했어요.',
};

function ReasonForm({ onSubmit }: { onSubmit: (reason: string) => Promise<boolean> }) {
  const close = useModalClose();
  const [reason, setReason] = useState('');
  const [busy, setBusy] = useState(false);
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!reason.trim() || busy) return;
    setBusy(true);
    const ok = await onSubmit(reason.trim());
    setBusy(false);
    if (ok) close();
  };
  return (
    <form onSubmit={submit}>
      <p className="aq-confirm-message">
        담당자가 확인한 뒤 이 발매가 나간 모든 플랫폼에 중단을 요청해요. 내려간 뒤에는 정산도 그 시점까지만 이어져요.
      </p>
      <div className="ui-field">
        <label htmlFor="takedownReason">중단하려는 이유 <span className="required">*</span></label>
        <textarea
          id="takedownReason" rows={4} maxLength={1000} required value={reason}
          onChange={e => setReason(e.target.value)}
          placeholder="예: 권리 계약이 끝났어요, 다른 유통사로 옮겨요"
        />
        <p className="ui-field-help aq-counter">{reason.length} / 1000</p>
      </div>
      <div className="aq-confirm-actions">
        <button type="button" className="ui-btn is-soft" onClick={close} disabled={busy}>닫기</button>
        <button type="submit" className="ui-btn is-danger" disabled={busy || !reason.trim()}>
          {busy ? '요청하는 중' : '중단 요청'}
        </button>
      </div>
    </form>
  );
}

export function TakedownCard({ releaseId }: { releaseId: string }) {
  const toast = useToast();
  const confirm = useConfirm();
  const [state, setState] = useState<TakedownState | null>(null);
  const [asking, setAsking] = useState(false);
  const load = useCallback(() => {
    api.takedownStatus(releaseId).then(setState).catch(() => setState(null));
  }, [releaseId]);
  useEffect(load, [load]);

  if (!state || (!state.platforms.length && !state.request)) return null;
  const req = state.request;

  const submit = async (reason: string) => {
    try {
      await api.requestTakedown(releaseId, reason);
      toast('발매 중단 요청을 접수했어요.', 'success');
      load();
      return true;
    } catch (e) {
      toast(errorMessage(e, '요청하지 못했어요. 잠시 후 다시 시도해 주세요.'), 'error');
      return false;
    }
  };
  const cancel = async () => {
    const ok = await confirm({ title: '중단 요청을 취소할까요?', message: '발매는 지금처럼 플랫폼에 그대로 남아요.', confirmLabel: '요청 취소' });
    if (!ok) return;
    try {
      await api.cancelTakedown(releaseId);
      load();
    } catch (e) {
      toast(errorMessage(e, '취소하지 못했어요.'), 'error');
    }
  };

  return (
    <div className="ui-card aq-takedown">
      <h2 className="subhead">플랫폼 서비스</h2>
      {!!state.platforms.length && (
        <ul className="aq-takedown-list">
          {state.platforms.map(p => (
            <li key={p.partner_id}>
              <span>{p.name || dspLabel(p.slug)}</span>
              <em className={p.live_status === 'TAKEN_DOWN' ? 'is-down' : p.live_status === 'TAKEDOWN_REQUESTED' ? 'is-pending' : ''}>{PLATFORM_STATE[p.live_status ?? 'UNKNOWN'] ?? '전송 완료'}</em>
            </li>
          ))}
        </ul>
      )}
      {req && (
        <p className="small muted aq-takedown-state">
          발매 중단 요청: {REQUEST_STATE[req.status]}
          {req.status === 'DECLINED' && req.decision_note ? ` ${req.decision_note}` : ''}
        </p>
      )}
      <div className="row-actions">
        {req?.status === 'PENDING' && (
          <button type="button" className="ui-btn is-soft" onClick={cancel}>중단 요청 취소</button>
        )}
        {state.can_request && (
          <button type="button" className="ui-btn is-text aq-danger-text" onClick={() => setAsking(true)}>발매 중단 요청</button>
        )}
      </div>
      {asking && (
        <Modal title="발매 중단 요청" modalClass="aq-confirm-mode" dismissible={false} onClose={() => setAsking(false)}>
          <ReasonForm onSubmit={submit} />
        </Modal>
      )}
    </div>
  );
}
