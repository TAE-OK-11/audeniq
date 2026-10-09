import type { DecisionAction, ReviewContext } from './api';

// An older API may be visible during a rollout. Keep reads usable and wait
// for authoritative action metadata before enabling any decision.
export function contextOrReadOnly(context: ReviewContext | undefined): ReviewContext {
  return context ?? { decision_kind: null, allowed_actions: [], requires_second_approval: false, pending_second_approval_id: null, check_counts: {} };
}

export function ReviewActions({ context, loading, onAction }: { context: ReviewContext; loading: boolean; onAction: (action: DecisionAction) => void }) {
  return <div className="adm-decide-actions">
    <button type="button" className="ui-btn is-primary" disabled={loading || !context.allowed_actions.includes('APPROVE')} onClick={() => onAction('APPROVE')}>승인</button>
    <button type="button" className="ui-btn is-warn" disabled={loading || !context.allowed_actions.includes('REQUEST_CORRECTION')} onClick={() => onAction('REQUEST_CORRECTION')}>보완 요청</button>
    <button type="button" className="ui-btn is-danger" disabled={loading || !context.allowed_actions.includes('REJECT')} onClick={() => onAction('REJECT')}>거절</button>
  </div>;
}
