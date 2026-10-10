// 업무 만들기·고치기 — 유형 · 우선순위 · 담당자 · 검토자 · 마감일 · 반복 · 연결 Entity.
// 발매 심사·문의 화면과 업무 상세(하위 업무)에서도 연결·상위 업무를 채워서 연다.
import { useState } from 'react';
import { useModalClose } from '../../components/Modal';
import { useToast } from '../../components/Toast';
import { errorMessage } from '../../api/errors';
import { Seg, useStaff } from '../ui';
import { workApi } from './api';
import { PRIORITIES, PRIORITY, RECUR, WORK_TYPE, WORK_TYPES } from './labels';
import { EntityChips, EntityPicker, PersonSelect, SwitchRow, usePeople } from './ui';
import type { EntityRef, Priority, RecurFreq, TaskDetail, TaskInput, WorkTask, WorkType } from './types';
import { DateTimeField } from '../../components/Picker';

/** datetime-local 값 ↔ ISO (브라우저 현지 시간) */
export const toLocalInput = (iso: string | null | undefined) => {
  if (!iso) return '';
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return '';
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
};
export const fromLocalInput = (v: string) => (v ? new Date(v).toISOString() : null);

export interface TaskFormInitial extends Partial<Omit<TaskInput, 'links'>> {
  /** 미리 연결할 Entity (이름 포함) */
  links?: EntityRef[];
  parentTitle?: string;
}

export function TaskForm({ task, initial, onSaved }: { task?: WorkTask; initial?: TaskFormInitial; onSaved: (d: TaskDetail) => void }) {
  const close = useModalClose();
  const toast = useToast();
  const { me } = useStaff();
  const people = usePeople();
  const src = task ?? initial ?? {};
  const [title, setTitle] = useState(src.title ?? '');
  const [description, setDescription] = useState(src.description ?? '');
  const [type, setType] = useState<WorkType>(src.type ?? 'GENERAL');
  const [priority, setPriority] = useState<Priority>(src.priority ?? 'NORMAL');
  const [assignee, setAssignee] = useState<string | null>(src.assignee_id !== undefined ? src.assignee_id ?? null : me.user_id);
  const [reviewer, setReviewer] = useState<string | null>(src.reviewer_id ?? null);
  const [due, setDue] = useState(toLocalInput(src.due_at));
  const isSub = !!(task?.parent_id ?? initial?.parent_id);
  const [recurOn, setRecurOn] = useState(!!src.recurrence);
  const [freq, setFreq] = useState<RecurFreq>(src.recurrence?.freq ?? 'WEEKLY');
  const [interval, setInterval] = useState(src.recurrence?.interval ?? 1);
  const [links, setLinks] = useState<EntityRef[]>(initial?.links ?? []);
  const [picking, setPicking] = useState(false);
  const [tags, setTags] = useState((src.tags ?? []).join(', '));
  const [busy, setBusy] = useState(false);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busy || !title.trim()) return;
    setBusy(true);
    const fields = {
      title: title.trim(), description: description.trim(), type, priority, assignee_id: assignee, reviewer_id: reviewer,
      due_at: fromLocalInput(due), recurrence: recurOn && !isSub ? { freq, interval: Math.max(1, Math.min(12, interval)) } : null,
      tags: tags.split(',').map(t => t.trim()).filter(Boolean).slice(0, 8),
    };
    try {
      const saved = task
        ? await workApi.updateTask(task.id, { ...fields, row_version: task.row_version })
        : await workApi.createTask({ ...fields, parent_id: initial?.parent_id ?? null, links: links.map(l => ({ entity_kind: l.kind, entity_id: l.id })) });
      toast(task ? '업무를 고쳤어요.' : `${saved.task.key} 업무를 만들었어요.`, 'success');
      onSaved(saved);
      close();
    } catch (err) {
      toast(errorMessage(err, '업무를 저장하지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };

  return (
    <form onSubmit={submit} className="adm-w-form">
      {initial?.parentTitle && <div className="ui-callout">하위 업무 · 상위: <b>{initial.parentTitle}</b></div>}
      <div className="ui-field">
        <label htmlFor="tTitle">제목 <span className="required">*</span></label>
        <input id="tTitle" data-autofocus className="ui-input" maxLength={200} value={title} onChange={e => setTitle(e.target.value)} placeholder="예: Lumi Records 계약 갱신 검토" />
      </div>
      <div className="adm-w-grid2">
        <div className="ui-field">
          <label htmlFor="tType">업무 유형</label>
          <select id="tType" className="ui-select" value={type} onChange={e => setType(e.target.value as WorkType)}>
            {WORK_TYPES.map(t => <option key={t} value={t}>{WORK_TYPE[t]}</option>)}
          </select>
        </div>
        <div className="ui-field">
          <label htmlFor="tDue">마감일</label>
          <DateTimeField id="tDue" className="ui-input" value={due} onChange={setDue} />
        </div>
      </div>
      <div className="ui-field">
        <span className="ui-field-label">우선순위</span>
        <Seg label="우선순위" value={priority} onChange={setPriority} options={PRIORITIES.map(p => [p, PRIORITY[p][0]] as const)} />
      </div>
      <div className="adm-w-grid2">
        <div className="ui-field">
          <label htmlFor="tAssignee">담당자</label>
          <PersonSelect id="tAssignee" value={assignee} onChange={setAssignee} people={people} empty="담당자 없음" />
        </div>
        <div className="ui-field">
          <label htmlFor="tReviewer">검토자</label>
          <PersonSelect id="tReviewer" value={reviewer} onChange={setReviewer} people={people} empty="검토 없이 완료" />
          <small>검토자가 있으면 완료 전에 승인을 받아요.</small>
        </div>
      </div>
      <div className="ui-field">
        <label htmlFor="tDesc">내용</label>
        <textarea id="tDesc" className="ui-textarea" rows={4} maxLength={8000} value={description} onChange={e => setDescription(e.target.value)} placeholder="무엇을, 어디까지 하면 끝나는지 적어 주세요." />
      </div>
      {!task && (
        <div className="ui-field">
          <span className="ui-field-label">연결</span>
          <EntityChips items={links} onRemove={r => setLinks(links.filter(x => !(x.kind === r.kind && x.id === r.id)))} />
          {picking
            ? <EntityPicker exclude={links.map(l => `${l.kind}:${l.id}`)} onPick={r => setLinks(list => [...list, r])} />
            : <button type="button" className="ui-btn is-soft is-sm adm-w-add" onClick={() => setPicking(true)}>발매·아티스트·계약·DSP 연결</button>}
        </div>
      )}
      {!isSub && (
        <div className="adm-w-box">
          <SwitchRow title="반복 업무" hint={recurOn ? '완료하면 다음 업무가 자동으로 만들어져요.' : '정기 점검·월말 정산 같은 일에 켜 주세요.'} checked={recurOn} onChange={setRecurOn} />
          {recurOn && (
            <div className="adm-w-recur">
              <Seg label="반복 주기" value={freq} onChange={setFreq} options={(['DAILY', 'WEEKLY', 'MONTHLY'] as const).map(f => [f, RECUR[f][0]] as const)} />
              <label className="adm-w-inline">
                <input type="number" className="ui-input" min={1} max={12} value={interval} onChange={e => setInterval(Number(e.target.value) || 1)} aria-label="반복 간격" />
                <span>{RECUR[freq][1]}마다</span>
              </label>
            </div>
          )}
        </div>
      )}
      <div className="ui-field" style={{ marginTop: 16 }}>
        <label htmlFor="tTags">태그</label>
        <input id="tTags" className="ui-input" value={tags} onChange={e => setTags(e.target.value)} placeholder="쉼표로 구분 (예: 월말, 긴급)" />
      </div>
      <div className="adm-form-actions">
        <button type="button" className="ui-btn is-soft" onClick={close}>취소</button>
        <button type="submit" className="ui-btn is-primary" disabled={busy || !title.trim()}>{busy ? '저장 중…' : task ? '저장' : '업무 만들기'}</button>
      </div>
    </form>
  );
}
