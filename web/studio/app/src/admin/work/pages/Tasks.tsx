// 업무 — 릴리즈 외 업무까지 전부 등록하는 범용 Task 엔진. 목록 / 보드로 보고, 담당·검토·유형으로 거른다.
import { useState } from 'react';
import { Link, useNavigate, useSearchParams } from '../../../lib/router';
import { Modal } from '../../../components/Modal';
import { useAsync } from '../../../hooks/useAsync';
import { Glyph } from '../../../components/Glyph';
import { Empty, ErrorBox, Filters, PageHead, Seg, Skeleton, useStaff } from '../../ui';
import { workApi, type TaskQuery } from '../api';
import { OPEN_STATUSES, TASK_STATUS, TASK_TYPE, TASK_TYPES, recurLabel } from '../labels';
import { TaskForm } from '../TaskForm';
import { Person, PersonSelect, PriorityChip, RelatedLinks, SlaChip, TaskStatusChip, dueOf, usePeople } from '../ui';
import type { EntityKind, TaskType, WorkTask } from '../types';
import { ENTITY_KIND } from '../labels';

const VIEWS = [['mine', '내 담당'], ['review', '내가 검토'], ['created', '내가 만든'], ['all', '전체']] as const;
type View = typeof VIEWS[number][0];

function TaskRow({ t, people }: { t: WorkTask; people: ReturnType<typeof usePeople> }) {
  return (
    <Link to={`/admin/tasks/${t.id}`} className={`adm-row adm-w-task-row${t.status === 'DONE' || t.status === 'CANCELLED' ? ' is-closed' : ''}`}>
      <Person id={t.assignee_id} people={people} compact />
      <span className="adm-min">
        <span className="adm-row-title"><span className="adm-w-key">{t.key}</span>{t.title}</span>
        <span className="adm-row-meta">
          <span>{TASK_TYPE[t.type]}</span>
          {t.reviewer_id && <span>검토 {people.get(t.reviewer_id)?.name ?? '지정됨'}</span>}
          {t.recurrence && <span className="adm-w-meta-icon"><Glyph name="repeat" size={12} />{recurLabel(t.recurrence)}</span>}
          {t.note_count > 0 && <span className="adm-w-meta-icon"><Glyph name="inquiry" size={12} />메모 {t.note_count}</span>}
        </span>
        {t.related.length > 0 && <RelatedLinks items={t.related.slice(0, 3)} plain />}
      </span>
      <span className="adm-row-end">
        <span className="adm-codes"><PriorityChip value={t.priority} raisedFrom={t.priority_raised_from} /><TaskStatusChip value={t.status} /></span>
        <SlaChip state={t.sla_state} due={dueOf(t)} short />
      </span>
    </Link>
  );
}

function Board({ items, people }: { items: WorkTask[]; people: ReturnType<typeof usePeople> }) {
  return (
    <div className="adm-w-board">
      {OPEN_STATUSES.map(s => {
        const col = items.filter(t => t.status === s);
        return (
          <section key={s} className="adm-w-col" aria-label={TASK_STATUS[s][0]}>
            <h3>{TASK_STATUS[s][0]}<b className="adm-count">{col.length}</b></h3>
            {col.length === 0 ? <p className="adm-w-col-empty">없음</p> : col.map(t => (
              <Link key={t.id} to={`/admin/tasks/${t.id}`} className={`adm-w-card${t.sla_state === 'OVERDUE' ? ' is-late' : ''}`}>
                <span className="adm-w-card-top"><span className="adm-w-key">{t.key}</span><PriorityChip value={t.priority} raisedFrom={t.priority_raised_from} /></span>
                <b>{t.title}</b>
                <span className="adm-w-card-foot">
                  <Person id={t.assignee_id} people={people} />
                  <SlaChip state={t.sla_state} due={dueOf(t)} short />
                </span>
              </Link>
            ))}
          </section>
        );
      })}
    </div>
  );
}

export function Tasks() {
  const nav = useNavigate();
  const [params, setParams] = useSearchParams();
  const view = (params.get('view') as View) || 'mine';
  const status = (params.get('status') as TaskQuery['status']) || 'open';
  const relKind = params.get('related_kind') as EntityKind | null;
  const relId = params.get('related_id');
  const [type, setType] = useState<TaskType | ''>('');
  const [assignee, setAssignee] = useState<string | null>(null);
  const [q, setQ] = useState('');
  const [layout, setLayout] = useState<'list' | 'board'>('list');
  const [creating, setCreating] = useState(false);
  const { refreshWork } = useStaff();
  const people = usePeople();
  const { data, loading, error, reload } = useAsync(
    () => workApi.tasks({ view, status: layout === 'board' ? 'open' : status, type, assignee: view === 'all' ? assignee ?? undefined : undefined, related_kind: relKind ?? undefined, related_id: relId ?? undefined }),
    [view, status, type, assignee, layout, relKind, relId],
  );
  const term = q.trim().toLowerCase();
  const items = (data?.items ?? []).filter(t => !term || [t.title, t.key, t.description, ...t.tags].some(v => v.toLowerCase().includes(term)));
  const set = (k: string, v: string) => { const next = new URLSearchParams(params); next.set(k, v); setParams(next, { replace: true }); };

  return (
    <div className="view-enter">
      <PageHead
        eyebrow="업무"
        title="업무"
        sub="계약 검토, DSP 문의, 정산 확인, 법무 대응까지 모든 일을 등록하고 담당자·검토자·기한으로 관리해요."
        actions={<>
          <button type="button" className="adm-btn soft small" onClick={reload}>새로고침</button>
          <button type="button" className="adm-btn primary small" onClick={() => setCreating(true)}><Glyph name="plus" size={14} />새 업무</button>
        </>}
      />
      <Filters label="담당" value={view} onChange={v => set('view', v)} options={VIEWS.map(([value, label]) => ({ value, label }))} />
      <div className="adm-queue-tools">
        <label htmlFor="tSearch" className="sr-only">검색</label>
        <input id="tSearch" type="search" className="adm-input" placeholder="제목·번호·태그로 찾기" value={q} onChange={e => setQ(e.target.value)} />
        <select className="adm-select" aria-label="업무 유형" value={type} onChange={e => setType(e.target.value as TaskType | '')}>
          <option value="">모든 유형</option>
          {TASK_TYPES.map(t => <option key={t} value={t}>{TASK_TYPE[t]}</option>)}
        </select>
        {view === 'all' && <PersonSelect id="tAssigneeFilter" label="담당자" value={assignee} onChange={setAssignee} people={people} empty="모든 담당자" />}
        {layout === 'list' && <Seg label="상태" value={status ?? 'open'} onChange={v => set('status', v)} options={[['open', '진행 중'], ['done', '완료'], ['all', '전체']] as const} />}
        <Seg label="보기 방식" value={layout} onChange={setLayout} options={[['list', '목록'], ['board', '보드']] as const} />
      </div>
      {relKind && relId && (
        <div className="adm-alert adm-alert-row">
          <span><b>{ENTITY_KIND[relKind]?.label ?? relKind} {relId}</b>에 연결된 업무만 보고 있어요.</span>
          <button type="button" className="adm-btn soft small" onClick={() => { const next = new URLSearchParams(params); next.delete('related_kind'); next.delete('related_id'); setParams(next, { replace: true }); }}>전체 보기</button>
        </div>
      )}
      {!loading && items.length > 0 && <p className="adm-queue-count">{items.length}건</p>}
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <Skeleton rows={5} /> : items.length === 0 ? (
        <Empty icon={<Glyph name="task" size={22} />} title={status === 'done' ? '완료한 업무가 없어요' : '진행 중인 업무가 없어요'}>
          <button type="button" className="adm-btn soft small" style={{ marginTop: 14 }} onClick={() => setCreating(true)}>새 업무 만들기</button>
        </Empty>
      ) : layout === 'board' ? <Board items={items} people={people} /> : (
        <div className="adm-list">{items.map(t => <TaskRow key={t.id} t={t} people={people} />)}</div>
      )}
      {creating && (
        <Modal title="새 업무" onClose={() => setCreating(false)} dismissible={false} modalClass="adm-w-modal">
          <TaskForm onSaved={t => { refreshWork(); nav(`/admin/tasks/${t.id}`); }} />
        </Modal>
      )}
    </div>
  );
}
