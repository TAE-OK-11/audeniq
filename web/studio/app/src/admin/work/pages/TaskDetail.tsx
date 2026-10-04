// 업무 상세 — 상태 진행(시작 → 검토 요청 → 승인/보완 → 완료), 담당·검토·우선순위·마감 바로 바꾸기, 내부 메모, 처리 이력.
import { useState } from 'react';
import { Link, useParams } from '../../../lib/router';
import { Modal, useModalClose } from '../../../components/Modal';
import { useToast } from '../../../components/Toast';
import { useConfirm } from '../../../components/Confirm';
import { errorMessage } from '../../../api/errors';
import { useAsync } from '../../../hooks/useAsync';
import { Glyph } from '../../../components/Glyph';
import { ErrorBox, Seg, Skeleton, useStaff } from '../../ui';
import { ago, when } from '../../labels';
import { workApi } from '../api';
import { EVENT_LABEL, PRIORITIES, PRIORITY, SLA_STATE, TASK_STATUS, TASK_TYPE, recurLabel } from '../labels';
import { remaining } from '../sla';
import { TaskForm, fromLocalInput, toLocalInput } from '../TaskForm';
import { NotesPanel, Person, PersonSelect, PriorityChip, RelatedLinks, SlaChip, TaskStatusChip, dueOf, usePeople } from '../ui';
import type { Priority, TaskPatch, TaskStatus, WorkTask } from '../types';

function ReviewForm({ task, approve, onDone }: { task: WorkTask; approve: boolean; onDone: () => void }) {
  const close = useModalClose();
  const toast = useToast();
  const [note, setNote] = useState('');
  const [busy, setBusy] = useState(false);
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busy || (!approve && !note.trim())) return;
    setBusy(true);
    try {
      await workApi.updateTask(task.id, { review: approve ? 'APPROVE' : 'REQUEST_CHANGES', review_note: note.trim() || undefined, row_version: task.row_version });
      toast(approve ? '승인하고 업무를 완료했어요.' : '보완을 요청했어요. 담당자 업무함에 들어가요.', 'success');
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
      <div className="adm-field">
        <label htmlFor="rvNote">{approve ? '남길 말 (선택)' : '보완할 내용'} {!approve && <span className="required">*</span>}</label>
        <textarea id="rvNote" data-autofocus className="adm-textarea" rows={4} maxLength={2000} value={note} onChange={e => setNote(e.target.value)} placeholder={approve ? '예: 조건 확인했어요.' : '예: 수수료 비교표에 지난 분기 조건도 넣어 주세요.'} />
      </div>
      <div className="adm-form-actions">
        <button type="button" className="adm-btn soft" onClick={close}>취소</button>
        <button type="submit" className={`adm-btn ${approve ? 'primary' : 'warn'}`} disabled={busy || (!approve && !note.trim())}>{busy ? '처리 중…' : approve ? '승인하고 완료' : '보완 요청'}</button>
      </div>
    </form>
  );
}

export function TaskDetail() {
  const { id = '' } = useParams<{ id: string }>();
  const toast = useToast();
  const confirm = useConfirm();
  const { me, refreshWork } = useStaff();
  const people = usePeople();
  const { data, loading, error, reload } = useAsync(() => workApi.task(id), [id]);
  const [editing, setEditing] = useState(false);
  const [review, setReview] = useState<null | 'approve' | 'changes'>(null);
  const [busy, setBusy] = useState(false);

  if (loading && !data) return <Skeleton rows={6} />;
  if (error || !data) return <ErrorBox message={error || '업무를 불러오지 못했어요.'} onRetry={reload} />;
  const t = data.task;
  const done = t.status === 'DONE' || t.status === 'CANCELLED';
  const iReview = t.reviewer_id === me.user_id;
  const due = dueOf(t);
  const idleMin = t.status === 'TODO' && t.assigned_at ? Math.round((Date.now() - Date.parse(t.assigned_at)) / 60000) : 0;

  const patch = async (p: Omit<TaskPatch, 'row_version'>, msg?: string) => {
    if (busy) return;
    setBusy(true);
    try {
      const saved = await workApi.updateTask(t.id, { ...p, row_version: t.row_version });
      if (msg) toast(saved.status === 'IN_REVIEW' && p.status === 'DONE' ? '검토자에게 승인을 요청했어요.' : msg, 'success');
      reload(); refreshWork();
    } catch (err) {
      toast(errorMessage(err, '바꾸지 못했어요. 새로고침 후 다시 시도해 주세요.'), 'error');
      reload();
    } finally {
      setBusy(false);
    }
  };
  const move = (s: TaskStatus, msg: string) => patch({ status: s }, msg);
  const cancel = async () => {
    if (await confirm({ title: '업무를 취소할까요?', message: '취소한 업무는 목록의 ‘완료’에서 다시 열 수 있어요.', confirmLabel: '업무 취소', danger: true })) move('CANCELLED', '업무를 취소했어요.');
  };

  return (
    <div className="view-enter">
      <div className="adm-detail-nav">
        <Link to="/admin/tasks" className="adm-back"><Glyph name="arrow-left" size={15} />업무 목록</Link>
        <div className="adm-detail-nav-end">
          <button type="button" className="adm-btn soft small" onClick={() => setEditing(true)}><Glyph name="pencil" size={14} />고치기</button>
        </div>
      </div>

      <div className="adm-detail">
        <div className="adm-min">
          <div className="adm-w-task-head">
            <span className="adm-codes"><span className="adm-w-key is-big">{t.key}</span><TaskStatusChip value={t.status} /><PriorityChip value={t.priority} raisedFrom={t.priority_raised_from} /><SlaChip state={t.sla_state} due={due} /></span>
            <h1>{t.title}</h1>
            <span className="adm-row-meta"><span>{TASK_TYPE[t.type]}</span><span>만든 사람 {people.get(t.created_by)?.name ?? '—'}</span><span>{when(t.created_at)}</span>{t.recurrence && <span className="adm-w-meta-icon"><Glyph name="repeat" size={12} />{recurLabel(t.recurrence)}</span>}</span>
          </div>

          {t.priority_raised_from && (
            <div className="adm-alert adm-w-alert-warn">기한이 가까워 우선순위가 <b>{PRIORITY[t.priority_raised_from][0]}</b>에서 <b>{PRIORITY[t.priority][0]}</b>(으)로 자동으로 올라갔어요.{t.escalation_level >= 2 && ' 책임자에게 escalation 됐어요.'}</div>
          )}
          {idleMin >= 60 && <div className="adm-alert">담당자가 맡은 지 <b>{idleMin >= 1440 ? `${Math.round(idleMin / 1440)}일` : `${Math.round(idleMin / 60)}시간`}</b> 동안 시작하지 않았어요.</div>}

          <section className="adm-card adm-section">
            <div className="adm-section-top"><h2>내용</h2></div>
            {t.description ? <p className="adm-w-desc">{t.description}</p> : <p className="muted small">내용이 없어요. ‘고치기’로 무엇을 하면 끝나는지 적어 주세요.</p>}
            {t.related.length > 0 && <div style={{ marginTop: 14 }}><RelatedLinks items={t.related} /></div>}
            {t.tags.length > 0 && <div className="adm-tags" style={{ marginTop: 12 }}>{t.tags.map(g => <span key={g}>#{g}</span>)}</div>}
          </section>

          <section className="adm-card adm-section">
            <NotesPanel kind="task" targetId={t.id} onChange={reload} />
          </section>

          <section className="adm-section">
            <div className="adm-section-top"><h2>처리 이력<small>{data.events.length}건</small></h2></div>
            <ol className="adm-timeline">
              {data.events.map((e, i) => (
                <li key={`${e.at}-${i}`} className={e.action === 'ESCALATED' || e.action === 'PRIORITY_RAISED' ? 'is-bad' : e.actor_id ? 'is-staff' : 'is-check'}>
                  <i aria-hidden="true" />
                  <div>
                    <div className="adm-tl-top"><b>{EVENT_LABEL[e.action] ?? e.action}</b>{e.action === 'STATUS' && e.detail && <TaskStatusChip value={e.detail as TaskStatus} />}</div>
                    {e.detail && e.action !== 'STATUS' && <p className="adm-tl-summary">{e.action === 'DUE' ? when(e.detail) : e.action === 'PRIORITY' ? PRIORITY[e.detail as Priority]?.[0] ?? e.detail : e.detail}</p>}
                    <small>{e.actor_id ? people.get(e.actor_id)?.name ?? '담당자' : '시스템'} · {ago(e.at)}</small>
                  </div>
                </li>
              ))}
            </ol>
          </section>
        </div>

        <aside className="adm-detail-side">
          <div className="adm-card adm-w-side">
            <h3>다음 할 일</h3>
            <div className="adm-w-actions">
              {t.status === 'TODO' && <button type="button" className="adm-btn primary" disabled={busy} onClick={() => move('IN_PROGRESS', '업무를 시작했어요.')}>시작하기</button>}
              {(t.status === 'IN_PROGRESS' || t.status === 'TODO') && (
                <button type="button" className={`adm-btn ${t.status === 'IN_PROGRESS' ? 'primary' : 'soft'}`} disabled={busy} onClick={() => move('DONE', '업무를 완료했어요.')}>
                  {t.reviewer_id && !iReview ? '검토 요청' : '완료'}
                </button>
              )}
              {t.status === 'IN_PROGRESS' && <button type="button" className="adm-btn warn" disabled={busy} onClick={() => move('BLOCKED', '막힘으로 표시했어요.')}>막힘</button>}
              {t.status === 'BLOCKED' && <button type="button" className="adm-btn primary" disabled={busy} onClick={() => move('IN_PROGRESS', '다시 진행해요.')}>다시 진행</button>}
              {t.status === 'IN_REVIEW' && iReview && <>
                <button type="button" className="adm-btn primary" disabled={busy} onClick={() => setReview('approve')}>승인</button>
                <button type="button" className="adm-btn warn" disabled={busy} onClick={() => setReview('changes')}>보완 요청</button>
              </>}
              {t.status === 'IN_REVIEW' && !iReview && <p className="small muted">{people.get(t.reviewer_id)?.name ?? '검토자'}님의 승인을 기다리고 있어요.</p>}
              {done && <button type="button" className="adm-btn soft" disabled={busy} onClick={() => move('IN_PROGRESS', '업무를 다시 열었어요.')}>다시 열기</button>}
            </div>
            {!done && <button type="button" className="adm-link-btn adm-w-cancel" onClick={cancel}>업무 취소</button>}
          </div>

          <div className="adm-card adm-w-side">
            <dl className="adm-w-fields">
              <div><dt><label htmlFor="dAssignee">담당자</label></dt><dd><PersonSelect id="dAssignee" value={t.assignee_id} onChange={v => patch({ assignee_id: v }, '담당자를 바꿨어요.')} people={people} empty="담당자 없음" /></dd></div>
              <div><dt><label htmlFor="dReviewer">검토자</label></dt><dd><PersonSelect id="dReviewer" value={t.reviewer_id} onChange={v => patch({ reviewer_id: v }, '검토자를 바꿨어요.')} people={people} empty="검토 없음" /></dd></div>
              <div><dt>우선순위</dt><dd><Seg label="우선순위" value={t.priority_raised_from ?? t.priority} onChange={v => patch({ priority: v }, '우선순위를 바꿨어요.')} options={PRIORITIES.map(p => [p, PRIORITY[p][0]] as const)} />{t.priority_raised_from && <small>기본 우선순위예요. 지금은 SLA로 {PRIORITY[t.priority][0]}까지 올라가 있어요.</small>}</dd></div>
              <div><dt><label htmlFor="dDue">마감일</label></dt><dd><input id="dDue" type="datetime-local" className="adm-input" defaultValue={toLocalInput(t.due_at)} key={t.due_at ?? 'none'} onBlur={e => { const v = fromLocalInput(e.target.value); if (v !== t.due_at) void patch({ due_at: v }, '마감일을 바꿨어요.'); }} /></dd></div>
              <div><dt>SLA 기한</dt><dd className="adm-w-sla-line">{t.sla_due_at ? <>{when(t.sla_due_at)}<small>{SLA_STATE[t.sla_state][0]}{!done && ` · ${remaining(t.sla_due_at)}`}</small></> : '적용 안 함'}</dd></div>
              <div><dt>담당 상태</dt><dd><Person id={t.assignee_id} people={people} /></dd></div>
              {t.completed_at && <div><dt>완료</dt><dd>{when(t.completed_at)}</dd></div>}
            </dl>
          </div>
          <p className="small muted adm-w-side-foot">상태 {TASK_STATUS[t.status][0]} · 마지막 변경 {ago(t.updated_at)}</p>
        </aside>
      </div>

      {editing && (
        <Modal title={`${t.key} 고치기`} onClose={() => setEditing(false)} dismissible={false} modalClass="adm-w-modal">
          <TaskForm task={t} onSaved={() => { reload(); refreshWork(); }} />
        </Modal>
      )}
      {review && (
        <Modal title={review === 'approve' ? '검토 승인' : '보완 요청'} onClose={() => setReview(null)} dismissible={false}>
          <ReviewForm task={t} approve={review === 'approve'} onDone={() => { reload(); refreshWork(); }} />
        </Modal>
      )}
    </div>
  );
}
