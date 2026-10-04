// 칸반 보드 — 업무를 상태 열(할 일 · 진행 중 · 검토 중 · 막힘 · 완료)로 보고, 끌어다 놓아 상태를 바꾼다.
// 상태 변경 규칙은 서버(업무 상세와 같은 API)가 정한다: 검토자가 있으면 '완료'는 '검토 중'으로,
// 하위 업무가 남았으면 완료할 수 없다. 키보드·터치에서는 카드의 '상태' 선택으로 옮긴다.
import { useEffect, useRef, useState } from 'react';
import { Link } from '../../lib/router';
import { useToast } from '../../components/Toast';
import { errorMessage } from '../../api/errors';
import { Glyph } from '../../components/Glyph';
import { useStaff } from '../ui';
import { workApi } from './api';
import { TASK_STATUS, WORK_TYPE } from './labels';
import { DueChip, Person, PriorityChip, type People } from './ui';
import type { TaskStatus, WorkTask } from './types';

export const BOARD_COLUMNS: TaskStatus[] = ['TODO', 'IN_PROGRESS', 'IN_REVIEW', 'BLOCKED', 'DONE'];
const DONE_DAYS = 7;

const MESSAGE: Partial<Record<TaskStatus, string>> = {
  TODO: '할 일로 옮겼어요.', IN_PROGRESS: '진행 중으로 옮겼어요.', IN_REVIEW: '검토를 요청했어요.', BLOCKED: '막힘으로 표시했어요.', DONE: '완료했어요.',
};

export function Kanban({ items, people, onReload }: { items: WorkTask[]; people: People; onReload: () => void }) {
  const toast = useToast();
  const { me } = useStaff();
  const [tasks, setTasks] = useState(items);
  const [dragging, setDragging] = useState<string | null>(null);
  const [over, setOver] = useState<TaskStatus | null>(null);
  const [adding, setAdding] = useState<TaskStatus | null>(null);
  const [title, setTitle] = useState('');
  const busy = useRef(new Set<string>());
  useEffect(() => { setTasks(items); }, [items]);

  const since = Date.now() - DONE_DAYS * 86_400_000;
  const visible = tasks.filter(t => !t.archived_at && t.status !== 'CANCELLED'
    && (t.status !== 'DONE' || (t.completed_at && Date.parse(t.completed_at) >= since)));

  const move = async (id: string, to: TaskStatus) => {
    const t = tasks.find(x => x.id === id);
    if (!t || t.status === to || busy.current.has(id)) return;
    busy.current.add(id);
    setTasks(xs => xs.map(x => (x.id === id ? { ...x, status: to, completed_at: to === 'DONE' ? new Date().toISOString() : x.completed_at } : x)));
    try {
      const d = await workApi.updateTask(id, { row_version: t.row_version, status: to });
      setTasks(xs => xs.map(x => (x.id === id ? d.task : x)));
      toast(d.task.status !== to && d.task.status === 'IN_REVIEW' ? '검토자가 있어 검토 요청으로 옮겼어요.' : MESSAGE[d.task.status] ?? '옮겼어요.', 'success');
    } catch (e) {
      setTasks(xs => xs.map(x => (x.id === id ? t : x)));
      toast(errorMessage(e, '옮기지 못했어요.'), 'error');
      onReload();
    } finally {
      busy.current.delete(id);
    }
  };

  const quickAdd = async (col: TaskStatus) => {
    const name = title.trim();
    if (!name) return;
    try {
      let d = await workApi.createTask({ title: name, type: 'GENERAL', priority: 'NORMAL', assignee_id: me.user_id });
      if (col !== 'TODO') d = await workApi.updateTask(d.task.id, { row_version: d.task.row_version, status: col });
      setTasks(xs => [...xs, d.task]);
      setTitle('');
      setAdding(null);
      toast(`${d.task.key}를 만들었어요.`, 'success');
    } catch (e) {
      toast(errorMessage(e, '만들지 못했어요.'), 'error');
    }
  };

  return (
    <div className="adm-kanban" role="list" aria-label="칸반 보드">
      {BOARD_COLUMNS.map(s => {
        const col = visible.filter(t => t.status === s);
        return (
          <section
            key={s} role="listitem" aria-label={`${TASK_STATUS[s][0]} ${col.length}건`}
            className={`adm-kanban-col is-${s.toLowerCase()}${over === s ? ' is-over' : ''}`}
            onDragOver={e => { if (dragging) { e.preventDefault(); e.dataTransfer.dropEffect = 'move'; if (over !== s) setOver(s); } }}
            onDragLeave={e => { if (!e.currentTarget.contains(e.relatedTarget as Node)) setOver(o => (o === s ? null : o)); }}
            onDrop={e => { e.preventDefault(); const id = e.dataTransfer.getData('text/plain') || dragging; setOver(null); setDragging(null); if (id) void move(id, s); }}
          >
            <h3>
              <span className={`adm-kanban-dot adm-t-${TASK_STATUS[s][1]}`} aria-hidden="true" />
              {TASK_STATUS[s][0]}<b className="adm-count">{col.length}</b>
              {s === 'DONE' && <small>최근 {DONE_DAYS}일</small>}
            </h3>
            <div className="adm-kanban-cards">
              {col.length === 0 && <p className="adm-w-col-empty">{dragging ? '여기에 놓기' : '없음'}</p>}
              {col.map(t => (
                <article
                  key={t.id}
                  className={`adm-kanban-card${t.overdue || t.sla_state === 'BREACHED' ? ' is-late' : ''}${dragging === t.id ? ' is-dragging' : ''}`}
                  draggable
                  onDragStart={e => { e.dataTransfer.setData('text/plain', t.id); e.dataTransfer.effectAllowed = 'move'; setDragging(t.id); }}
                  onDragEnd={() => { setDragging(null); setOver(null); }}
                >
                  <span className="adm-w-card-top"><span className="adm-w-key">{t.key}</span><PriorityChip value={t.priority} /></span>
                  <Link to={`/admin/tasks/${t.id}`} className="adm-kanban-title" draggable={false}>{t.title}</Link>
                  <span className="adm-kanban-meta">
                    <span>{WORK_TYPE[t.type]}</span>
                    {t.subtasks.total > 0 && <span><Glyph name="task" size={11} /> {t.subtasks.done}/{t.subtasks.total}</span>}
                    {t.note_count > 0 && <span><Glyph name="inquiry" size={11} /> {t.note_count}</span>}
                    {(t.escalation_level ?? 0) > 0 && <span className="is-late-text">상향</span>}
                  </span>
                  <span className="adm-w-card-foot">
                    <Person id={t.assignee_id} people={people} />
                    <DueChip due={t.deadline ?? t.due_at} closed={t.status === 'DONE'} />
                  </span>
                  <label className="adm-kanban-move">
                    <span className="sr-only">{t.key} 상태 바꾸기</span>
                    <select className="adm-select" value={t.status} onChange={e => void move(t.id, e.target.value as TaskStatus)}>
                      {BOARD_COLUMNS.map(c => <option key={c} value={c}>{TASK_STATUS[c][0]}</option>)}
                    </select>
                  </label>
                </article>
              ))}
            </div>
            {s !== 'DONE' && (adding === s ? (
              <form className="adm-kanban-add" onSubmit={e => { e.preventDefault(); void quickAdd(s); }}>
                <label htmlFor={`kAdd-${s}`} className="sr-only">새 업무 제목</label>
                <input id={`kAdd-${s}`} className="adm-input" autoFocus maxLength={200} value={title} onChange={e => setTitle(e.target.value)}
                  placeholder="업무 제목" onKeyDown={e => { if (e.key === 'Escape') { setAdding(null); setTitle(''); } }} />
                <span className="adm-w-btns">
                  <button type="submit" className="adm-btn primary small" disabled={!title.trim()}>추가</button>
                  <button type="button" className="adm-btn soft small" onClick={() => { setAdding(null); setTitle(''); }}>취소</button>
                </span>
              </form>
            ) : (
              <button type="button" className="adm-kanban-addbtn" onClick={() => { setAdding(s); setTitle(''); }}><Glyph name="plus" size={13} />업무 추가</button>
            ))}
          </section>
        );
      })}
    </div>
  );
}
