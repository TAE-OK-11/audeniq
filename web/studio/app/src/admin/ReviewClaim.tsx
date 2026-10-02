// 심사 담당 — 결정(승인·보완 요청·거절)은 이 발매를 맡은 담당자만 할 수 있다.
// 아무도 맡지 않았으면 ‘담당하기’, 내가 맡았으면 ‘담당 해제’, 다른 담당자가 맡았으면 누구인지(ADMIN은 넘겨받기).
import { useState } from 'react';
import type { ReviewContext } from './api';
import { when } from './labels';

export function claimLabel(context: ReviewContext): string {
  const c = context.claim;
  if (!c) return '담당자 없음';
  return c.mine ? '내 담당' : `${c.email.split('@')[0]} 담당`;
}

export function ReviewClaimPanel({ context, canReview, onClaim, onRelease, compact = false }: {
  context: ReviewContext;
  canReview: boolean;
  onClaim: (takeOver: boolean) => Promise<void>;
  onRelease: () => Promise<void>;
  /** 아래 고정 바 안: 한 줄로 */
  compact?: boolean;
}) {
  const [busy, setBusy] = useState(false);
  const run = async (fn: () => Promise<void>) => {
    if (busy) return;
    setBusy(true);
    try { await fn(); } finally { setBusy(false); }
  };
  const c = context.claim;
  if (!context.decision_kind) return null;

  if (!c) {
    return (
      <div className={`adm-claim is-free${compact ? ' is-compact' : ''}`}>
        {!compact && <p>아직 담당자가 없어요. 담당하면 이 심사의 결정은 나만 할 수 있어요.</p>}
        {canReview && context.can_claim !== false && (
          <button type="button" className="adm-btn primary adm-claim-go" disabled={busy} onClick={() => void run(() => onClaim(false))}>
            {busy ? '담당하는 중…' : '이 심사 담당하기'}
          </button>
        )}
      </div>
    );
  }
  if (c.mine) {
    return (
      <div className={`adm-claim is-mine${compact ? ' is-compact' : ''}`}>
        <span className="adm-claim-who"><i aria-hidden="true" />내 담당{!compact && <small> · {when(c.at)}부터</small>}</span>
        {!compact && <button type="button" className="adm-claim-link" disabled={busy} onClick={() => void run(onRelease)}>담당 해제</button>}
      </div>
    );
  }
  return (
    <div className={`adm-claim is-other${compact ? ' is-compact' : ''}`}>
      <span className="adm-claim-who"><i aria-hidden="true" /><b>{c.email}</b> 담당 중{!compact && <small> · {when(c.at)}부터</small>}</span>
      {!compact && <p>담당자만 결정할 수 있어요.</p>}
      {context.can_take_over && (
        <button type="button" className="adm-btn soft small" disabled={busy} onClick={() => void run(() => onClaim(true))}>넘겨받기</button>
      )}
    </div>
  );
}
