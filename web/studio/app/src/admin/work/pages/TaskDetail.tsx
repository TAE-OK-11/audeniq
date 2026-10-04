// 업무 상세 — 상태 진행(시작 → 검토 요청 → 승인/보완 → 완료), 하위 업무, 연결 Entity,
// 담당·검토·우선순위·마감 바로 바꾸기, 내부 메모, 처리 이력, 보관.
import { useState } from 'react';
import { Link, useNavigate, useParams } from '../../../lib/router';
import { Modal, useModalClose } from '../../../components/Modal';
import { useToast } from '../../../components/Toast';
import { useConfirm } from '../../../components/Confirm';
import { errorMessage } from '../../../api/errors';
import { useAsync } from '../../../hooks/useAsync';
import { Glyph } from '../../../components/Glyph';
import { Chip, ErrorBox, Seg, Skeleton, useStaff } from '../../ui';
import { ago, when } from '../../labels';
import { workApi } from '../api';
import { ENTITY_KIND, EVENT_LABEL, PRIORITIES, PRIORITY, SLA_STATE, TASK_STATUS, WORK_TYPE, recurLabel, remaining } from '../labels';
import { TaskForm, fromLocalInput, toLocalInput } from '../TaskForm';
import { DueChip, EntityChips, EntityPicker, NotesPanel, Person, PersonSelect, PriorityChip, TaskStatusChip, usePeople, type People } from '../ui';
import type { EntityKind, Priority, TaskDetail as Detail, TaskEvent, TaskPatch, TaskStatus, WorkTask } from '../types';

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
      toast(approve ? '승인하고 업무를 완료했어요.' : '보완을 요청했어요.', 'success');
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

/** 이력 한 줄의 detail을 사람이 읽는 말로 */
function eventDetail(e: TaskEvent, people: People): string | null {
  if (!e.detail) return null;
  switch (e.action) {
    case 'DUE': return when(e.detail);
    case 'PRIORITY': return PRIORITY[e.detail as Priority]?.[0] ?? e.detail;
    case 'TYPE': return WORK_TYPE[e.detail as keyof typeof WORK_TYPE] ?? e.detail;
    case 'ASSIGNED': case 'REVIEWER': return people.get(e.detail)?.email ?? '지정됨';
    case 'LINKED': case 'UNLINKED': {
      const [kind] = e.detail.split(':');
      return ENTITY_KIND[kind as EntityKind]?.label ?? kind;
    }
    case 'EDITED': return e.detail === 'title' ? '제목' : '내용';
    default: return e.detail;
  }
}

function Subtasks({ d, people, onChanged }: { d: Detail; people: People; onChanged: () => void }) {
  const [adding, setAdding] = useState(false);
  const t = d.task;
  return (
    <section className="adm-card adm-section">
      <div className="adm-section-top">
        <h2>하위 업무<small>{t.subtasks.done}/{t.subtasks.total} 완료</small></h2>
        {!t.archived_at && <button type="button" className="adm-btn soft small" onClick={() => setAdding(true)}><Glyph name="plus" size={14} />하위 업무</button>}
      </div>
      {d.subtasks.length === 0 ? <p className="small muted">하위 업무가 없어요. 큰 일을 나눠 담당자를 따로 정할 수 있어요.</p> : (
        <div className="adm-list">
          {d.subtasks.map(s => (
            <Link key={s.id} to={`/admin/tasks/${s.id}`} className={`adm-row adm-w-task-row is-compact${s.status === 'DONE' || s.status === 'CANCELLED' ? ' is-closed' : ''}`}>
              <Person id={s.assignee_id} people={people} compact />
              <span className="adm-min"><span className="adm-row-title"><span className="adm-w-key">{s.key}</span>{s.title}</span></span>
              <span className="adm-codes"><DueChip due={s.due_at} closed={s.status === 'DONE'} /><TaskStatusChip value={s.status} /></span>
            </Link>
          ))}
        </div>
      )}
      {adding && (
        <Modal title="하위 업무" onClose={() => setAdding(false)} dismissible={false} modalClass="adm-w-modal">
          <TaskForm initial={{ parent_id: t.id, parentTitle: `${t.key} ${t.title}`, type: t.type, priority: t.priority }} onSaved={onChanged} />
        </Modal>
      )}
    </section>
  );
}

export function TaskDetail() {
  const { id = '' } = useParams<{ id: string }>();
  const nav = useNavigate();
  const toast = useToast();
  const confirm = useConfirm();
  const { me } = useStaff();
  const people = usePeople();
  const { data, loading, error, reload } = useAsync(() => workApi.task(id), [id]);
  const [editing, setEditing] = useState(false);
  const [review, setReview] = useState<null | 'approve' | 'changes'>(null);
  const [linking, setLinking] = useState(false);
  const [busy, setBusy] = useState(false);

  if (loading && !data) return <Skeleton rows={6} />;
  if (error || !data) return <ErrorBox message={error || '업무를 불러오지 못했어요.'} onRetry={reload} />;
  const t = data.task;
  const closed = t.status === 'DONE' || t.status === 'CANCELLED';
  const archived = !!t.archived_at;
  const iReview = t.reviewer_id === me.user_id || me.role === 'ADMIN';
  const canArchive = !archived && (t.created_by === me.user_id || t.assignee_id === me.user_id || me.role === 'ADMIN');
  const openSubs = t.subtasks.total - t.subtasks.done;

  const run = async (fn: () => Promise<unknown>, msg?: string) => {
    if (busy) return;
    setBusy(true);
    try {
      await fn();
      if (msg) toast(msg, 'success');
      reload();
    } catch (err) {
      toast(errorMessage(err, '바꾸지 못했어요. 새로고침 후 다시 시도해 주세요.'), 'error');
      reload();
    } finally {
      setBusy(false);
    }
  };
  const patch = (p: Omit<TaskPatch, 'row_version'>, msg?: string) => run(() => workApi.updateTask(t.id, { ...p, row_version: t.row_version }), msg);
  const move = (s: TaskStatus, msg: string) => run(async () => {
    const r = await workApi.updateTask(t.id, { status: s, row_version: t.row_version });
    if (s === 'DONE' && r.task.status === 'IN_REVIEW') toast('검토자에게 승인을 요청했어요.', 'success');
    else toast(msg, 'success');
  });
  const cancel = async () => {
    if (await confirm({ title: '업무를 취소할까요?', message: '취소한 업무는 ‘완료’ 목록에서 다시 열 수 있어요.', confirmLabel: '업무 취소', danger: true })) void move('CANCELLED', '업무를 취소했어요.');
  };
  const archive = async () => {
    const message = t.subtasks.total ? `하위 업무 ${t.subtasks.total}개도 함께 보관돼요. 기록은 남아요.` : '목록에서 사라지지만 기록은 남아요.';
    if (await confirm({ title: '업무를 보관할까요?', message, confirmLabel: '보관', danger: true })) {
      void run(async () => { await workApi.archiveTask(t.id); toast('업무를 보관했어요.', 'success'); nav('/admin/tasks'); });
    }
  };

  return (
    <div className="view-enter">
      <div className="adm-detail-nav">
        <Link to={data.parent ? `/admin/tasks/${data.parent.id}` : '/admin/tasks'} className="adm-back"><Glyph name="arrow-left" size={15} />{data.parent ? `${data.parent.key} ${data.parent.title}` : '업무 목록'}</Link>
        {!archived && (
          <div className="adm-detail-nav-end">
            <button type="button" className="adm-btn soft small" onClick={() => setEditing(true)}><Glyph name="pencil" size={14} />고치기</button>
          </div>
        )}
      </div>

      <div className="adm-detail">
        <div className="adm-min">
          <div className="adm-w-task-head">
            <span className="adm-codes"><span className="adm-w-key is-big">{t.key}</span><TaskStatusChip value={t.status} /><PriorityChip value={t.priority} /><DueChip due={t.deadline ?? t.due_at} closed={closed} />{(t.escalation_level ?? 0) > 0 && <Chip tone="red">SLA 상향 {t.escalation_level}단계</Chip>}</span>
            <h1>{t.title}</h1>
            <span className="adm-row-meta"><span>{WORK_TYPE[t.type]}</span><span>만든 사람 {people.get(t.created_by)?.email ?? '—'}</span><span>{when(t.created_at)}</span>{t.recurrence && <span className="adm-w-meta-icon"><Glyph name="repeat" size={12} />{recurLabel(t.recurrence)}</span>}</span>
          </div>

          {archived && <div className="adm-alert">보관한 업무예요 ({when(t.archived_at)}). 내용과 기록은 그대로 남아 있어요.</div>}
          {t.changes_requested && !closed && <div className="adm-alert adm-w-alert-warn">검토자가 보완을 요청했어요. 처리 이력에서 내용을 확인해 주세요.</div>}

          <section className="adm-card adm-section">
            <div className="adm-section-top"><h2>내용</h2></div>
            {t.description ? <p className="adm-w-desc">{t.description}</p> : <p className="muted small">내용이 없어요. ‘고치기’로 무엇을 하면 끝나는지 적어 주세요.</p>}
            {t.tags.length > 0 && <div className="adm-tags" style={{ marginTop: 12 }}>{t.tags.map(g => <span key={g}>#{g}</span>)}</div>}
          </section>

          <section className="adm-card adm-section">
            <div className="adm-section-top">
              <h2>연결<small>발매·아티스트·계약·DSP·정산 등</small></h2>
              {!archived && !linking && <button type="button" className="adm-btn soft small" onClick={() => setLinking(true)}><Glyph name="plus" size={14} />연결</button>}
            </div>
            {t.links.length === 0 && !linking && <p className="small muted">연결한 항목이 없어요.</p>}
            <EntityChips items={t.links} onRemove={archived ? undefined : r => r.link_id && run(() => workApi.unlink(r.link_id!), '연결을 해제했어요.')} />
            {linking && (
              <div style={{ marginTop: 10 }}>
                <EntityPicker exclude={t.links.map(l => `${l.kind}:${l.id}`)} onPick={r => run(() => workApi.linkTask(t.id, { entity_kind: r.kind, entity_id: r.id }), '연결했어요.')} />
                <div className="adm-form-actions"><button type="button" className="adm-btn soft small" onClick={() => setLinking(false)}>닫기</button></div>
              </div>
            )}
          </section>

          {!t.parent_id && <Subtasks d={data} people={people} onChanged={reload} />}

          <section className="adm-card adm-section">
            <NotesPanel kind="task" targetId={t.id} onChange={reload} />
          </section>

          <section className="adm-section">
            <div className="adm-section-top"><h2>처리 이력<small>{data.events.length}건</small></h2></div>
            <ol className="adm-timeline">
              {data.events.map((e, i) => {
                const detail = eventDetail(e, people);
                return (
                  <li key={`${e.at}-${i}`} className={e.action === 'CHANGES_REQUESTED' || e.action === 'ARCHIVED' ? 'is-warn' : e.actor_id ? 'is-staff' : 'is-check'}>
                    <i aria-hidden="true" />
                    <div>
                      <div className="adm-tl-top"><b>{EVENT_LABEL[e.action] ?? e.action}</b>{(e.action === 'STATUS' || e.action === 'REVIEW_REQUESTED') && e.detail && <TaskStatusChip value={e.detail as TaskStatus} />}</div>
                      {detail && e.action !== 'STATUS' && e.action !== 'REVIEW_REQUESTED' && <p className="adm-tl-summary">{detail}</p>}
                      <small>{e.actor_id ? people.get(e.actor_id)?.email ?? '담당자' : '시스템'} · {ago(e.at)}</small>
                    </div>
                  </li>
                );
              })}
            </ol>
          </section>
        </div>

        <aside className="adm-detail-side">
          {!archived && (
            <div className="adm-card adm-w-side">
              <h3>다음 할 일</h3>
              <div className="adm-w-actions">
                {t.status === 'TODO' && <button type="button" className="adm-btn primary" disabled={busy} onClick={() => move('IN_PROGRESS', '업무를 시작했어요.')}>시작하기</button>}
                {(t.status === 'IN_PROGRESS' || t.status === 'TODO') && (
                  <button type="button" className={`adm-btn ${t.status === 'IN_PROGRESS' ? 'primary' : 'soft'}`} disabled={busy || (openSubs > 0 && !(t.reviewer_id && t.reviewer_id !== me.user_id))} onClick={() => move('DONE', '업무를 완료했어요.')}>
                    {t.reviewer_id && t.reviewer_id !== me.user_id ? '검토 요청' : '완료'}
                  </button>
                )}
                {openSubs > 0 && (t.status === 'IN_PROGRESS' || t.status === 'TODO' || t.status === 'IN_REVIEW') && <p className="small muted">하위 업무 {openSubs}개가 끝나야 완료할 수 있어요.</p>}
                {t.status === 'IN_PROGRESS' && <button type="button" className="adm-btn warn" disabled={busy} onClick={() => move('BLOCKED', '막힘으로 표시했어요.')}>막힘</button>}
                {t.status === 'BLOCKED' && <button type="button" className="adm-btn primary" disabled={busy} onClick={() => move('IN_PROGRESS', '다시 진행해요.')}>다시 진행</button>}
                {t.status === 'IN_REVIEW' && iReview && <>
                  <button type="button" className="adm-btn primary" disabled={busy || openSubs > 0} onClick={() => setReview('approve')}>승인</button>
                  <button type="button" className="adm-btn warn" disabled={busy} onClick={() => setReview('changes')}>보완 요청</button>
                </>}
                {t.status === 'IN_REVIEW' && !iReview && <p className="small muted">{people.get(t.reviewer_id)?.email ?? '검토자'}의 승인을 기다리고 있어요.</p>}
                {closed && <button type="button" className="adm-btn soft" disabled={busy} onClick={() => move('IN_PROGRESS', '업무를 다시 열었어요.')}>다시 열기</button>}
              </div>
              <span className="adm-w-side-links">
                {!closed && <button type="button" className="adm-link-btn" onClick={cancel}>업무 취소</button>}
                {canArchive && <button type="button" className="adm-link-btn" onClick={archive}>보관</button>}
              </span>
            </div>
          )}

          <div className="adm-card adm-w-side">
            <dl className="adm-w-fields">
              <div><dt><label htmlFor="dAssignee">담당자</label></dt><dd>{archived ? <Person id={t.assignee_id} people={people} /> : <PersonSelect id="dAssignee" value={t.assignee_id} onChange={v => patch({ assignee_id: v }, '담당자를 바꿨어요.')} people={people} empty="담당자 없음" />}</dd></div>
              <div><dt><label htmlFor="dReviewer">검토자</label></dt><dd>{archived ? <Person id={t.reviewer_id} people={people} empty="검토 없음" /> : <PersonSelect id="dReviewer" value={t.reviewer_id} onChange={v => patch({ reviewer_id: v }, '검토자를 바꿨어요.')} people={people} empty="검토 없음" />}</dd></div>
              <div><dt>우선순위</dt><dd>{archived ? <PriorityChip value={t.priority} /> : <Seg label="우선순위" value={t.priority} onChange={v => patch({ priority: v }, '우선순위를 바꿨어요.')} options={PRIORITIES.map(p => [p, PRIORITY[p][0]] as const)} />}</dd></div>
              {t.sla_due_at && <div><dt>SLA 기한</dt><dd>{when(t.sla_due_at)}{t.sla_state && <small>{SLA_STATE[t.sla_state][0]} · {remaining(t.sla_due_at)}{t.priority_raised ? ` · 우선순위 ${PRIORITY[t.priority_raised][0]}에서 올라감` : ''}</small>}</dd></div>}
              <div><dt><label htmlFor="dDue">마감일</label></dt><dd>{archived ? (t.due_at ? when(t.due_at) : '—') : <input id="dDue" type="datetime-local" className="adm-input" defaultValue={toLocalInput(t.due_at)} key={t.due_at ?? 'none'} onBlur={e => { const v = fromLocalInput(e.target.value); if (v !== t.due_at) void patch({ due_at: v }, '마감일을 바꿨어요.'); }} />}</dd></div>
              {t.started_at && <div><dt>시작</dt><dd>{when(t.started_at)}</dd></div>}
              {t.completed_at && <div><dt>{t.status === 'CANCELLED' ? '취소' : '완료'}</dt><dd>{when(t.completed_at)}</dd></div>}
            </dl>
          </div>
          <p className="small muted adm-w-side-foot">상태 {TASK_STATUS[t.status][0]} · 마지막 변경 {ago(t.updated_at)}</p>
        </aside>
      </div>

      {editing && (
        <Modal title={`${t.key} 고치기`} onClose={() => setEditing(false)} dismissible={false} modalClass="adm-w-modal">
          <TaskForm task={t} onSaved={reload} />
        </Modal>
      )}
      {review && (
        <Modal title={review === 'approve' ? '검토 승인' : '보완 요청'} onClose={() => setReview(null)} dismissible={false}>
          <ReviewForm task={t} approve={review === 'approve'} onDone={reload} />
        </Modal>
      )}
    </div>
  );
}
