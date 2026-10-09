// 카드 상세 서랍 — 보드를 떠나지 않고 카드 내용·사람·마감·태그를 고치고, 연결·문서·메모·이력을 본다.
// 저장은 row_version과 함께 보내고, 다른 곳에서 먼저 바뀌었으면(409) 최신 내용을 다시 불러와 알린다.
// 열고 닫을 때 포커스: 열면 제목 칸, 닫으면 그 카드로 돌아간다. Esc로 닫고 Tab은 서랍 안에서 돈다.
import { useCallback, useEffect, useId, useRef, useState, type FormEvent } from 'react';
import { createPortal } from 'react-dom';
import { Link, useNavigate } from '../../../lib/router';
import { useToast } from '../../../components/Toast';
import { useConfirm } from '../../../components/Confirm';
import { errorMessage } from '../../../api/errors';
import { Glyph } from '../../../components/Glyph';
import { ErrorBox, SkeletonList, useStaff } from '../../ui';
import { ago, when } from '../../labels';
import { workApi } from '../api';
import { ENTITY_KIND, EVENT_LABEL, LINK_KINDS, PRIORITIES, PRIORITY, PRIORITY_CODE, PRIORITY_NAME, TASK_STATUS, WORK_TYPE, WORK_TYPES } from '../labels';
import { fromLocalInput, toLocalInput } from '../TaskForm';
import { EntityChips, EntityPicker, NotesPanel, Person, PersonSelect, TaskStatusChip, usePeople, type People } from '../ui';
import { sourceLabel } from './Card';
import { cardSelector, errorCode } from './logic';
import type { EntityKind, Priority, TaskDetail, TaskEvent, TaskPatch, TaskStatus, WorkTask, WorkType } from '../types';
import { DateTimeField } from '../../../components/Picker';

interface Draft { title: string; description: string; priority: Priority; type: WorkType; assignee_id: string | null; reviewer_id: string | null; due: string; tags: string }
const draftOf = (t: WorkTask): Draft => ({
  title: t.title, description: t.description, priority: t.priority, type: t.type, assignee_id: t.assignee_id, reviewer_id: t.reviewer_id,
  due: toLocalInput(t.due_at), tags: t.tags.join(', '),
});
const tagList = (v: string) => [...new Set(v.split(',').map(x => x.trim().replace(/^#/, '')).filter(Boolean))].slice(0, 8);

/** 바뀐 칸만 보낸다 */
function patchOf(t: WorkTask, d: Draft): Omit<TaskPatch, 'row_version'> {
  const p: Omit<TaskPatch, 'row_version'> = {};
  if (d.title.trim() !== t.title) p.title = d.title.trim();
  if (d.description.trim() !== t.description) p.description = d.description.trim();
  if (d.priority !== t.priority) p.priority = d.priority;
  if (d.type !== t.type) p.type = d.type;
  if (d.assignee_id !== t.assignee_id) p.assignee_id = d.assignee_id;
  if (d.reviewer_id !== t.reviewer_id) p.reviewer_id = d.reviewer_id;
  if (d.due !== toLocalInput(t.due_at)) p.due_at = fromLocalInput(d.due);
  const tags = tagList(d.tags);
  if (JSON.stringify(tags) !== JSON.stringify(t.tags)) p.tags = tags;
  return p;
}

function eventText(e: TaskEvent, people: People): string | null {
  if (!e.detail) return null;
  switch (e.action) {
    case 'STATUS': case 'REVIEW_REQUESTED': return TASK_STATUS[e.detail as TaskStatus]?.[0] ?? e.detail;
    case 'PRIORITY': return PRIORITY[e.detail as Priority] ? `${PRIORITY_CODE[e.detail as Priority]} ${PRIORITY_NAME[e.detail as Priority]}` : e.detail;
    case 'TYPE': return WORK_TYPE[e.detail as WorkType] ?? e.detail;
    case 'DUE': return when(e.detail);
    case 'ASSIGNED': case 'REVIEWER': return people.get(e.detail)?.email ?? '지정됨';
    case 'LINKED': case 'UNLINKED': return ENTITY_KIND[e.detail.split(':')[0] as EntityKind]?.label ?? null;
    case 'EDITED': return null;
    default: return e.detail;
  }
}

const FOCUSABLE = 'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

export function CardDrawer({ cardId, boardWritable, onClose, onChanged, onArchived }: {
  cardId: string;
  boardWritable: boolean;
  onClose: () => void;
  /** 보드의 카드를 서버 값으로 바꾼다 */
  onChanged: (t: WorkTask) => void;
  onArchived: (id: string) => void;
}) {
  const toast = useToast();
  const confirm = useConfirm();
  const nav = useNavigate();
  const { me } = useStaff();
  const people = usePeople();
  const titleId = useId();
  const panel = useRef<HTMLElement>(null);
  const titleInput = useRef<HTMLInputElement>(null);
  const [detail, setDetail] = useState<TaskDetail | null>(null);
  const [error, setError] = useState('');
  const [draft, setDraft] = useState<Draft | null>(null);
  const [busy, setBusy] = useState(false);
  const [linking, setLinking] = useState(false);
  const focused = useRef(false);
  const onChangedRef = useRef(onChanged);
  onChangedRef.current = onChanged;
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  const show = useCallback((d: TaskDetail, resetDraft = true) => {
    setDetail(d);
    if (resetDraft) setDraft(draftOf(d.task));
    onChangedRef.current(d.task);
  }, []);
  const load = useCallback(async (resetDraft = true) => {
    setError('');
    try { show(await workApi.task(cardId), resetDraft); } catch (e) { setError(errorMessage(e, '카드를 불러오지 못했어요.')); }
  }, [cardId, show]);

  useEffect(() => {
    setDetail(null); setDraft(null); setLinking(false); focused.current = false;
    void load();
  }, [load]);

  // 처음 불러오면 제목 칸으로
  useEffect(() => {
    if (draft && !focused.current) { focused.current = true; titleInput.current?.focus({ preventScroll: true }); }
  }, [draft]);

  // 열 때: 서랍으로 포커스 / 닫을 때: 카드로 돌아간다. Esc 닫기 · Tab 가두기
  useEffect(() => {
    const prev = document.activeElement as HTMLElement | null;
    panel.current?.focus({ preventScroll: true });
    const onKey = (e: KeyboardEvent) => {
      // 확인 대화상자 같은 모달이 위에 있으면 그쪽이 처리한다
      if (document.getElementById('modal')) return;
      if (e.key === 'Escape' && !e.defaultPrevented) { e.preventDefault(); onCloseRef.current(); return; }
      if (e.key !== 'Tab' || !panel.current) return;
      const list = Array.from(panel.current.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(el => el.offsetParent !== null || el === document.activeElement);
      if (list.length === 0) return;
      const first = list[0];
      const last = list[list.length - 1];
      if (e.shiftKey && (document.activeElement === first || document.activeElement === panel.current)) { e.preventDefault(); last.focus(); }
      else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
    };
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('keydown', onKey);
      const card = document.querySelector<HTMLElement>(cardSelector(cardId));
      (card ?? (prev && document.contains(prev) ? prev : null))?.focus({ preventScroll: true });
    };
  }, [cardId]);

  const t = detail?.task;
  const archived = !!t?.archived_at;
  const mine = !!t && [t.assignee_id, t.reviewer_id, t.created_by].includes(me.user_id);
  const editable = !!t && !archived && (boardWritable || mine);
  const patch = t && draft ? patchOf(t, draft) : {};
  const dirty = Object.keys(patch).length > 0;

  const run = async (fn: () => Promise<TaskDetail | void>, msg?: string) => {
    if (busy) return;
    setBusy(true);
    try {
      const d = await fn();
      if (d) show(d);
      if (msg) toast(msg, 'success');
    } catch (e) {
      if (errorCode(e) === 'CONFLICT') {
        toast('다른 곳에서 먼저 바뀐 카드예요. 최신 내용을 다시 불러왔어요. 고친 내용을 다시 입력해 주세요.', 'error');
        void load();
      } else {
        toast(errorMessage(e, '저장하지 못했어요.'), 'error');
      }
    } finally {
      setBusy(false);
    }
  };
  const save = (e: FormEvent) => {
    e.preventDefault();
    if (!t || !draft || !dirty || !editable) return;
    if (!draft.title.trim()) { toast('제목을 입력해 주세요.', 'error'); return; }
    void run(() => workApi.updateTask(t.id, { ...patch, row_version: t.row_version }), '카드를 저장했어요.');
  };
  const archive = async () => {
    if (!t) return;
    const message = t.subtasks.total ? `하위 업무 ${t.subtasks.total}개도 함께 보관돼요. 기록은 남아요.` : '보드와 목록에서 사라지지만 기록은 남아요.';
    if (!(await confirm({ title: `${t.key} 카드를 보관할까요?`, message, confirmLabel: '보관', danger: true }))) return;
    void run(async () => {
      await workApi.archiveTask(t.id);
      toast('카드를 보관했어요.', 'success');
      onArchived(t.id);
    });
  };
  const newDoc = () => {
    if (!t) return;
    void run(async () => {
      const d = await workApi.createDoc({ title: t.title, link: { entity_kind: 'task', entity_id: t.id } });
      toast('카드에 연결된 문서를 만들었어요.', 'success');
      nav(`/admin/docs/${d.doc.id}`);
    });
  };
  const set = <K extends keyof Draft>(k: K, v: Draft[K]) => setDraft(d => (d ? { ...d, [k]: v } : d));

  const body = (
    <div className="adm-kb-drawer-wrap">
      <div className="adm-kb-drawer-backdrop" onMouseDown={() => onClose()} aria-hidden="true" />
      <aside ref={panel} className="adm-kb-drawer" role="dialog" aria-modal="true" aria-labelledby={titleId} tabIndex={-1}>
        <div className="adm-kb-drawer-top">
          <h2 id={titleId}>{t ? <><span className="ui-tag is-mono">{t.key}</span><span className="sr-only"> {t.title}</span></> : '카드'}</h2>
          <span className="adm-w-btns">
            {t && <Link to={`/admin/tasks/${t.id}`} className="ui-btn is-soft is-sm">전체 화면<Glyph name="chevron-right" size={13} /></Link>}
            <button type="button" className="adm-kb-icon-btn" aria-label="닫기" onClick={onClose}><Glyph name="close" size={18} /></button>
          </span>
        </div>
        <div className="adm-kb-drawer-body">
          {error ? <ErrorBox message={error} onRetry={() => void load()} /> : !t || !draft ? <SkeletonList rows={5} /> : (
            <>
              {archived && <div className="ui-callout">보관한 카드예요 ({when(t.archived_at)}). 내용과 기록은 그대로 남아 있어요.</div>}
              {!editable && !archived && <div className="ui-callout">이 카드는 보기만 할 수 있어요. 바꾸려면 보드 쓰기 권한이 있거나 카드 담당·검토자여야 해요.</div>}
              {t.changes_requested && <div className="ui-callout is-warn">검토자가 보완을 요청했어요. 아래 이력에서 내용을 확인해 주세요.</div>}
              <form className="adm-kb-form" onSubmit={save}>
                <div className="ui-field">
                  <label htmlFor="kbdTitle">제목</label>
                  <input id="kbdTitle" ref={titleInput} className="ui-input" maxLength={200} value={draft.title} disabled={!editable} onChange={e => set('title', e.target.value)} />
                </div>
                <div className="ui-field">
                  <label htmlFor="kbdDesc">내용</label>
                  <textarea id="kbdDesc" className="ui-textarea" rows={4} maxLength={10000} value={draft.description} disabled={!editable} placeholder="무엇을 하면 끝나는지 적어 주세요." onChange={e => set('description', e.target.value)} />
                </div>
                <div className="adm-w-grid2">
                  <div className="ui-field">
                    <label htmlFor="kbdPriority">우선순위</label>
                    <select id="kbdPriority" className="ui-select" value={draft.priority} disabled={!editable} onChange={e => set('priority', e.target.value as Priority)}>
                      {PRIORITIES.map(p => <option key={p} value={p}>{PRIORITY_CODE[p]} · {PRIORITY_NAME[p]} ({PRIORITY[p][0]})</option>)}
                    </select>
                  </div>
                  <div className="ui-field">
                    <label htmlFor="kbdType">유형</label>
                    <select id="kbdType" className="ui-select" value={draft.type} disabled={!editable} onChange={e => set('type', e.target.value as WorkType)}>
                      {WORK_TYPES.map(w => <option key={w} value={w}>{WORK_TYPE[w]}</option>)}
                    </select>
                  </div>
                  <div className="ui-field">
                    <label htmlFor="kbdAssignee">담당자</label>
                    {editable ? <PersonSelect id="kbdAssignee" value={draft.assignee_id} onChange={v => set('assignee_id', v)} people={people} empty="담당자 없음" /> : <Person id={t.assignee_id} people={people} />}
                  </div>
                  <div className="ui-field">
                    <label htmlFor="kbdReviewer">검토자</label>
                    {editable ? <PersonSelect id="kbdReviewer" value={draft.reviewer_id} onChange={v => set('reviewer_id', v)} people={people} empty="검토 없음" /> : <Person id={t.reviewer_id} people={people} empty="검토 없음" />}
                  </div>
                  <div className="ui-field">
                    <label htmlFor="kbdDue">마감일</label>
                    <DateTimeField id="kbdDue" className="ui-input" value={draft.due} disabled={!editable} onChange={v => set('due', v)} />
                  </div>
                  <div className="ui-field">
                    <label htmlFor="kbdTags">태그</label>
                    <input id="kbdTags" className="ui-input" maxLength={300} value={draft.tags} disabled={!editable} placeholder="쉼표로 나눠요 (8개까지)" onChange={e => set('tags', e.target.value)} />
                  </div>
                </div>
                {editable && (
                  <div className="adm-kb-form-actions">
                    {dirty && <small className="muted">저장하지 않은 변경이 있어요.</small>}
                    <button type="button" className="ui-btn is-soft is-sm" disabled={!dirty || busy} onClick={() => setDraft(draftOf(t))}>되돌리기</button>
                    <button type="submit" className="ui-btn is-primary is-sm" disabled={!dirty || busy}>{busy ? '저장 중…' : '저장'}</button>
                  </div>
                )}
              </form>

              <dl className="adm-w-fields adm-kb-facts">
                <div><dt>열 · 상태</dt><dd><span>{t.column_name ?? '—'}</span> <TaskStatusChip value={t.status} /></dd></div>
                <div><dt>출처</dt><dd>{sourceLabel(t.source) ?? '직접 등록'}</dd></div>
                <div><dt>만든 사람</dt><dd>{t.created_by ? <Person id={t.created_by} people={people} /> : '시스템 (자동 카드)'}</dd></div>
                <div><dt>만든 때</dt><dd>{when(t.created_at)}</dd></div>
                <div><dt>마지막 변경</dt><dd>{when(t.updated_at)} <small>{ago(t.updated_at)}</small></dd></div>
                {t.completed_at && <div><dt>{t.status === 'CANCELLED' ? '취소' : '완료'}</dt><dd>{when(t.completed_at)}</dd></div>}
                {t.subtasks.total > 0 && <div><dt>하위 업무</dt><dd>{t.subtasks.done}/{t.subtasks.total} 완료</dd></div>}
              </dl>

              <section className="adm-kb-drawer-sec" aria-labelledby="kbdLinks">
                <div className="ui-section-head">
                  <h3 id="kbdLinks">연결<small>발매·아티스트·계약·문서 등</small></h3>
                  {editable && (
                    <span className="adm-w-btns">
                      <button type="button" className="ui-btn is-soft is-sm" disabled={busy} onClick={newDoc}><Glyph name="doc" size={13} />새 문서 만들기</button>
                      {!linking && <button type="button" className="ui-btn is-soft is-sm" onClick={() => setLinking(true)}><Glyph name="plus" size={13} />연결</button>}
                    </span>
                  )}
                </div>
                {t.links.length === 0 && !linking && <p className="small muted">연결한 항목이 없어요.</p>}
                <EntityChips items={t.links} onRemove={editable ? r => { if (r.link_id) void run(() => workApi.unlink(r.link_id!), '연결을 해제했어요.'); } : undefined} />
                {linking && (
                  <div className="adm-kb-picker">
                    <EntityPicker kinds={LINK_KINDS} exclude={t.links.map(l => `${l.kind}:${l.id}`)} onPick={r => void run(() => workApi.linkTask(t.id, { entity_kind: r.kind, entity_id: r.id }), '연결했어요.')} />
                    <div className="adm-form-actions"><button type="button" className="ui-btn is-soft is-sm" onClick={() => setLinking(false)}>닫기</button></div>
                  </div>
                )}
              </section>

              <section className="adm-kb-drawer-sec">
                <NotesPanel kind="task" targetId={t.id} title="댓글 · 내부 메모" onChange={() => void load(false)} />
              </section>

              <section className="adm-kb-drawer-sec" aria-labelledby="kbdHistory">
                <div className="ui-section-head"><h3 id="kbdHistory">활동<small>{detail.events.length}건</small></h3></div>
                {detail.events.length === 0 ? <p className="small muted">기록이 없어요.</p> : (
                  <ol className="adm-kb-events">
                    {detail.events.slice(0, 40).map((e, i) => {
                      const text = eventText(e, people);
                      return (
                        <li key={`${e.at}-${i}`}>
                          <b>{EVENT_LABEL[e.action] ?? e.action}</b>{text && <span> · {text}</span>}
                          <small>{e.actor_id ? people.get(e.actor_id)?.email ?? '담당자' : '시스템'} · <time dateTime={e.at} title={when(e.at)}>{ago(e.at)}</time></small>
                        </li>
                      );
                    })}
                  </ol>
                )}
              </section>

              {editable && (
                <div className="adm-kb-drawer-foot">
                  <button type="button" className="ui-btn is-danger is-sm" disabled={busy} onClick={() => void archive()}>카드 보관</button>
                </div>
              )}
            </>
          )}
        </div>
      </aside>
    </div>
  );
  return createPortal(body, document.getElementById('root') ?? document.body);
}
