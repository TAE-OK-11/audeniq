// 업무 — 릴리즈 외 업무까지 전부 등록하는 범용 Task 엔진. 목록으로 보고(보드는 /admin/board),
// 담당·상태·유형·우선순위·담당자·기한·연결 Entity로 거른다.
import { useState } from 'react';
import { Link, useNavigate, useSearchParams } from '../../../lib/router';
import { Modal } from '../../../components/Modal';
import { useAsync } from '../../../hooks/useAsync';
import { Glyph } from '../../../components/Glyph';
import { EmptyState, ErrorBox, ChipGroup, PageHeader, Seg, SkeletonList, WorkTabs } from '../../ui';
import { workApi } from '../api';
import { ENTITY_KIND, PRIORITIES, PRIORITY, WORK_TYPE, WORK_TYPES, recurLabel } from '../labels';
import { TaskForm } from '../TaskForm';
import { DueChip, EntityChips, Person, PersonSelect, PriorityChip, TaskStatusChip, usePeople, type People } from '../ui';
import type { EntityKind, Priority, TaskQuery, WorkTask, WorkType } from '../types';

const VIEWS = [['mine', '내 담당'], ['review', '내가 검토'], ['created', '내가 만든'], ['all', '전체']] as const;
type View = typeof VIEWS[number][0];
const closed = (t: WorkTask) => t.status === 'DONE' || t.status === 'CANCELLED';

function TaskRow({ t, people }: { t: WorkTask; people: People }) {
  return (
    <Link to={`/admin/tasks/${t.id}`} className={`ui-row adm-w-task-row${closed(t) || t.archived_at ? ' is-closed' : ''}`}>
      <Person id={t.assignee_id} people={people} compact />
      <span className="adm-min">
        <span className="ui-row-title"><span className="ui-tag is-mono">{t.key}</span>{t.title}</span>
        <span className="ui-row-meta">
          <span>{WORK_TYPE[t.type]}</span>
          {t.reviewer_id && <span>검토 {people.get(t.reviewer_id)?.name ?? '지정됨'}</span>}
          {t.subtasks.total > 0 && <span className="adm-w-meta-icon"><Glyph name="task" size={12} />하위 {t.subtasks.done}/{t.subtasks.total}</span>}
          {t.recurrence && <span className="adm-w-meta-icon"><Glyph name="repeat" size={12} />{recurLabel(t.recurrence)}</span>}
          {t.note_count > 0 && <span className="adm-w-meta-icon"><Glyph name="inquiry" size={12} />메모 {t.note_count}</span>}
        </span>
        {t.links.length > 0 && <EntityChips items={t.links.slice(0, 3)} plain />}
      </span>
      <span className="ui-row-end">
        <span className="adm-codes"><PriorityChip value={t.priority} /><TaskStatusChip value={t.status} /></span>
        <DueChip due={t.deadline ?? t.due_at} closed={closed(t)} />
      </span>
    </Link>
  );
}

export function Tasks() {
  const nav = useNavigate();
  const [params, setParams] = useSearchParams();
  const view = (params.get('view') as View) || 'mine';
  const status = (params.get('status') as NonNullable<TaskQuery['status']>) || 'open';
  const entityKind = (params.get('entity_kind') as EntityKind | null) ?? '';
  const entityId = params.get('entity_id') ?? '';
  const [type, setType] = useState<WorkType | ''>('');
  const [priority, setPriority] = useState<Priority | ''>('');
  const [due, setDue] = useState<NonNullable<TaskQuery['due']>>('');
  const [assignee, setAssignee] = useState<string | null>(null);
  const [q, setQ] = useState('');
  // 보드 보기는 칸반 보드 화면으로 (보드마다 열·필터가 따로 있다)
  const setLayout = (v: 'list' | 'board') => { if (v === 'board') nav('/admin/board'); };
  const [creating, setCreating] = useState(false);
  const people = usePeople();
  const { data, loading, error, reload } = useAsync(
    () => workApi.tasks({
      view, status, type, priority, due, q: q.trim() || undefined,
      assignee: view === 'all' ? assignee ?? undefined : undefined, entity_kind: entityKind, entity_id: entityId,
    }),
    [view, status, type, priority, due, assignee, entityKind, entityId, q],
  );
  const items = data?.items ?? [];
  const set = (k: string, v: string) => { const next = new URLSearchParams(params); next.set(k, v); setParams(next, { replace: true }); };
  const clearEntity = () => { const next = new URLSearchParams(params); next.delete('entity_kind'); next.delete('entity_id'); setParams(next, { replace: true }); };

  return (
    <div className="view-enter">
      <PageHeader
        eyebrow="업무"
        title="업무"
        sub="계약 검토, DSP 문의, 정산 확인, 법무 대응까지 모든 일을 등록하고 담당자·검토자·기한으로 관리해요."
        actions={<>
          <button type="button" className="ui-btn is-soft is-sm" onClick={reload}>새로고침</button>
          <button type="button" className="ui-btn is-primary is-sm" onClick={() => setCreating(true)}><Glyph name="plus" size={14} />새 업무</button>
        </>}
      />
      <WorkTabs />
      <ChipGroup label="담당" value={view} onChange={v => set('view', v)} options={VIEWS.map(([value, label]) => ({ value, label }))} />
      {entityKind && entityId && (
        <div className="ui-callout is-row">
          <span><b>{ENTITY_KIND[entityKind]?.label ?? entityKind}</b>에 연결된 업무만 보고 있어요.</span>
          <button type="button" className="ui-btn is-soft is-sm" onClick={clearEntity}>전체 보기</button>
        </div>
      )}
      <div className="adm-queue-tools">
        <label htmlFor="tSearch" className="sr-only">검색</label>
        <input id="tSearch" type="search" className="ui-input" placeholder="제목·번호·태그로 찾기" value={q} onChange={e => setQ(e.target.value)} />
        <Seg label="보기 방식" value="list" onChange={setLayout} options={[['list', '목록'], ['board', '보드']] as const} />
      </div>
      <div className="adm-queue-tools adm-w-filter-row">
        <Seg label="상태" value={status} onChange={v => set('status', v)} options={[['open', '진행 중'], ['done', '완료'], ['all', '전체'], ['archived', '보관']] as const} />
        <select className="ui-select is-auto" aria-label="업무 유형" value={type} onChange={e => setType(e.target.value as WorkType | '')}>
          <option value="">모든 유형</option>
          {WORK_TYPES.map(t => <option key={t} value={t}>{WORK_TYPE[t]}</option>)}
        </select>
        <select className="ui-select is-auto" aria-label="우선순위" value={priority} onChange={e => setPriority(e.target.value as Priority | '')}>
          <option value="">모든 우선순위</option>
          {PRIORITIES.map(p => <option key={p} value={p}>{PRIORITY[p][0]}</option>)}
        </select>
        <select className="ui-select is-auto" aria-label="기한" value={due} onChange={e => setDue(e.target.value as NonNullable<TaskQuery['due']>)}>
          <option value="">모든 기한</option>
          <option value="overdue">기한 지남</option>
          <option value="today">오늘까지</option>
          <option value="week">7일 안</option>
        </select>
        {view === 'all' && <PersonSelect id="tAssigneeFilter" label="담당자" value={assignee} onChange={setAssignee} people={people} empty="모든 담당자" />}
      </div>
      {!loading && items.length > 0 && <p className="adm-queue-count">{items.length}건</p>}
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <SkeletonList rows={5} /> : items.length === 0 ? (
        <EmptyState icon={<Glyph name="task" size={22} />} title={status === 'archived' ? '보관한 업무가 없어요' : status === 'done' ? '완료한 업무가 없어요' : '진행 중인 업무가 없어요'}>
          <button type="button" className="ui-btn is-soft is-sm" style={{ marginTop: 14 }} onClick={() => setCreating(true)}>새 업무 만들기</button>
        </EmptyState>
      ) : (
        <div className="ui-list">{items.map(t => <TaskRow key={t.id} t={t} people={people} />)}</div>
      )}
      {creating && (
        <Modal title="새 업무" onClose={() => setCreating(false)} dismissible={false} modalClass="adm-w-modal">
          <TaskForm onSaved={d => nav(`/admin/tasks/${d.task.id}`)} />
        </Modal>
      )}
    </div>
  );
}
