// 발매 후 요청 — 배급 중단·계약 종료·유통사 이전.
// 요청 → (서버가 확인한) 권한·계약 → 다른 담당자 승인 → 플랫폼별 내림 → 플랫폼 확인 또는 담당자 증빙으로 완료.
// 플랫폼이 실제로 내렸는지는 이 시스템 밖의 일이라, 플랫폼 통지나 담당자가 확인한 증빙만 기록한다.
import { useState } from 'react';
import { Link } from '../../lib/router';
import { Modal, useModalClose } from '../../components/Modal';
import { useToast } from '../../components/Toast';
import { errorMessage } from '../../api/errors';
import { useAsync } from '../../hooks/useAsync';
import { staffApi, type ReleaseActionItem, type ReleaseActionTask } from '../api';
import { ago } from '../labels';
import { DSP_NAME } from '../dspReqs';
import { Badge, EmptyState, ErrorBox, ChipGroup, NoDuty, PageHeader, SkeletonList, DELIVERY_TABS, SubTabs, useStaff } from '../ui';
import { Glyph } from '../../components/Glyph';

const KIND: Record<ReleaseActionItem['kind'], string> = { TAKEDOWN: '배급 중단', TERMINATION: '계약 종료', TRANSFER: '유통사 이전' };
const STATUS: Record<string, string> = { REQUESTED: '승인 대기', IN_PROGRESS: '진행 중', COMPLETED: '완료', REJECTED: '반려', CANCELLED: '취소' };
const TASK: Record<string, { label: string; tone: 'green' | 'red' | 'amber' | 'gray' }> = {
  QUEUED: { label: '전송 대기', tone: 'gray' },
  SENT: { label: '플랫폼 접수 · 확인 대기', tone: 'amber' },
  MANUAL_REQUIRED: { label: '연동 없음 · 플랫폼 채널에서 직접 처리', tone: 'red' },
  FAILED: { label: '실패', tone: 'red' },
  CONFIRMED: { label: '내려감', tone: 'green' },
};
const VIEWS = [{ value: 'open', label: '처리할 요청' }, { value: 'all', label: '전체' }] as const;

function DecisionForm({ action, approve, onDone }: { action: ReleaseActionItem; approve: boolean; onDone: () => void }) {
  const close = useModalClose();
  const toast = useToast();
  const [note, setNote] = useState('');
  const [busy, setBusy] = useState(false);
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busy || (!approve && !note.trim())) return;
    setBusy(true);
    try {
      await staffApi.decideReleaseAction(action.id, { approve, note: note.trim() });
      toast(approve ? '승인했어요. 플랫폼별로 내리기 시작해요.' : '반려했어요.', 'success');
      onDone();
      close();
    } catch (err) {
      toast(errorMessage(err, '처리하지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };
  return (
    <form onSubmit={submit}>
      <p className="small muted">
        {action.release_title} · {KIND[action.kind]}
        {approve ? (action.kind === 'TRANSFER' ? ` — ${action.effective_date}에 모든 플랫폼에서 내려요.` : ' — 지금 올라가 있는 플랫폼마다 내림 요청을 보내요.') : ' — 아티스트에게 반려 사유가 알림으로 가요.'}
      </p>
      <div className="ui-field" style={{ marginTop: 14 }}>
        <label htmlFor="raNote">{approve ? '메모' : '반려 사유'}{!approve && <span className="required"> *</span>}</label>
        <textarea id="raNote" data-autofocus className="ui-input" rows={3} maxLength={2000} value={note} onChange={e => setNote(e.target.value)} />
      </div>
      <div className="adm-form-actions">
        <button type="button" className="ui-btn is-soft" onClick={close}>취소</button>
        <button type="submit" className={`ui-btn ${approve ? 'is-primary' : 'is-warn'}`} disabled={busy || (!approve && !note.trim())}>
          {busy ? '처리 중…' : approve ? '승인' : '반려'}
        </button>
      </div>
    </form>
  );
}

function EvidenceForm({ action, task, onDone }: { action: ReleaseActionItem; task: ReleaseActionTask; onDone: () => void }) {
  const close = useModalClose();
  const toast = useToast();
  const [note, setNote] = useState('');
  const [busy, setBusy] = useState(false);
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busy || !note.trim()) return;
    setBusy(true);
    try {
      await staffApi.releaseActionEvidence(action.id, task.id, note.trim());
      toast('내림 확인을 기록했어요.', 'success');
      onDone();
      close();
    } catch (err) {
      toast(errorMessage(err, '기록하지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };
  return (
    <form onSubmit={submit}>
      <p className="small muted">{action.release_title} · {DSP_NAME[task.partner_id] ?? task.partner_id}에서 내려간 것을 직접 확인했을 때만 기록해 주세요.</p>
      <div className="ui-field" style={{ marginTop: 14 }}>
        <label htmlFor="raEvidence">확인 근거 <span className="required">*</span></label>
        <textarea id="raEvidence" data-autofocus className="ui-input" rows={3} maxLength={1000} value={note} onChange={e => setNote(e.target.value)} placeholder="예: 파트너 포털에서 삭제 확인, 담당자 메일 회신(날짜)" />
      </div>
      <div className="adm-form-actions">
        <button type="button" className="ui-btn is-soft" onClick={close}>취소</button>
        <button type="submit" className="ui-btn is-primary" disabled={busy || !note.trim()}>{busy ? '기록 중…' : '내림 확인 기록'}</button>
      </div>
    </form>
  );
}

export function ReleaseActions() {
  const toast = useToast();
  const { can, me } = useStaff();
  const [view, setView] = useState<'open' | 'all'>('open');
  const { data, loading, error, reload } = useAsync(() => staffApi.releaseActions(view === 'all'), [view]);
  const [decision, setDecision] = useState<{ action: ReleaseActionItem; approve: boolean } | null>(null);
  const [evidence, setEvidence] = useState<{ action: ReleaseActionItem; task: ReleaseActionTask } | null>(null);
  const actions = data?.actions ?? [];

  const retry = async (a: ReleaseActionItem, t: ReleaseActionTask) => {
    try {
      await staffApi.retryReleaseAction(a.id, t.id);
      toast('다시 보냈어요. 잠시 후 새로고침해 주세요.', 'success');
      reload();
    } catch (e) {
      toast(errorMessage(e, '다시 보내지 못했어요.'), 'error');
    }
  };

  return (
    <div className="view-enter">
      <PageHeader
        eyebrow="배급"
        title="발매 후 요청"
        sub="배급 중단·계약 종료·유통사 이전 요청이에요. 요청한 사람이 아닌 담당자가 승인하고, 플랫폼마다 내려간 것을 확인해야 끝나요."
        actions={<button type="button" className="ui-btn is-soft is-sm" onClick={reload}>새로고침</button>}
      />
      <SubTabs tabs={DELIVERY_TABS} />
      {!can('DELIVERY') && <NoDuty duty="배급 관리" />}
      <ChipGroup label="보기" value={view} onChange={v => setView(v as 'open' | 'all')} options={VIEWS.map(v => ({ value: v.value, label: v.label }))} />
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <SkeletonList rows={4} /> : actions.length === 0 ? (
        <EmptyState icon={<Glyph name="truck" size={22} />} title="처리할 요청이 없어요">아티스트가 배급 중단·이전을 요청하면 여기에 모여요.</EmptyState>
      ) : (
        <div className="ui-list">
          {actions.map(a => {
            const mine = a.requested_by === me?.user_id;
            return (
              <div key={a.id} className="ui-card adm-deliv ui-on-soft">
                <div className="adm-check-top">
                  <span className="adm-min">
                    <Link to={`/admin/reviews/${a.release_id}`} className="ui-row-title">{a.release_title} <Glyph name="arrow-right" size={13} className="aq-inline-glyph" /></Link>
                    <span className="ui-row-meta">
                      <span>{KIND[a.kind]}{a.transfer_to ? ` → ${a.transfer_to}` : ''}{a.effective_date ? ` · ${a.effective_date}` : ''}</span>
                      <span>{a.source === 'STAFF' ? '담당자 접수' : '아티스트(소유자) 요청'}</span>
                      <span>{ago(a.created_at)}</span>
                    </span>
                  </span>
                  <span className="adm-codes">
                    <Badge tone={a.status === 'COMPLETED' ? 'green' : a.status === 'REJECTED' ? 'red' : a.status === 'REQUESTED' ? 'amber' : 'gray'}>{STATUS[a.status] ?? a.status}</Badge>
                  </span>
                </div>
                <p className="small break">{a.reason}</p>
                {a.decision_note && <p className="small muted break">메모: {a.decision_note}</p>}
                {a.tasks.length > 0 && (
                  <ul className="adm-deliv-rows">
                    {a.tasks.map(t => {
                      const st = TASK[t.status] ?? { label: t.status, tone: 'gray' as const };
                      return (
                        <li key={t.id} className={`is-${st.tone}`}>
                          <b className="adm-deliv-name">{DSP_NAME[t.partner_id] ?? t.partner_id}</b>
                          <span className="adm-min">
                            <span className="adm-deliv-head">{st.label}{t.confirmed_via === 'STAFF_EVIDENCE' ? ' (담당자 증빙)' : t.confirmed_via === 'PARTNER' ? ' (플랫폼 통지)' : ''}</span>
                            {t.evidence_note && <small className="adm-deliv-note">{t.evidence_note}</small>}
                            {t.last_error && t.status !== 'CONFIRMED' && <small className="adm-deliv-problem">{t.last_error}</small>}
                          </span>
                          {can('DELIVERY') && a.status === 'IN_PROGRESS' && t.status !== 'CONFIRMED' && (
                            <span className="adm-codes">
                              {(t.status === 'FAILED' || t.status === 'MANUAL_REQUIRED') && (
                                <button type="button" className="ui-btn is-text is-muted" onClick={() => retry(a, t)}>다시 보내기</button>
                              )}
                              <button type="button" className="ui-btn is-soft is-sm" onClick={() => setEvidence({ action: a, task: t })}>내림 확인 기록</button>
                            </span>
                          )}
                        </li>
                      );
                    })}
                  </ul>
                )}
                {can('DELIVERY') && a.status === 'REQUESTED' && (
                  <div className="adm-form-actions">
                    {mine ? <span className="small muted">직접 접수한 요청은 다른 담당자가 승인해요.</span> : (
                      <>
                        <button type="button" className="ui-btn is-soft" onClick={() => setDecision({ action: a, approve: false })}>반려</button>
                        <button type="button" className="ui-btn is-primary" onClick={() => setDecision({ action: a, approve: true })}>승인</button>
                      </>
                    )}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
      {decision && (
        <Modal title={decision.approve ? '요청 승인' : '요청 반려'} onClose={() => setDecision(null)} dismissible={false}>
          <DecisionForm action={decision.action} approve={decision.approve} onDone={reload} />
        </Modal>
      )}
      {evidence && (
        <Modal title="내림 확인 기록" onClose={() => setEvidence(null)} dismissible={false}>
          <EvidenceForm action={evidence.action} task={evidence.task} onDone={reload} />
        </Modal>
      )}
    </div>
  );
}
