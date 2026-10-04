// 업무 만들기·고치기 — 유형 · 우선순위 · 담당자 · 검토자 · 마감일 · 반복 · 연결 항목.
// 예외 처리·거래처·발매 심사 화면에서도 연결 항목을 채워서 연다.
import { useEffect, useState } from 'react';
import { useModalClose } from '../../components/Modal';
import { useToast } from '../../components/Toast';
import { errorMessage } from '../../api/errors';
import { Glyph } from '../../components/Glyph';
import { Seg, useStaff } from '../ui';
import { staffApi } from '../api';
import { DSP_NAME } from '../dspReqs';
import { workApi } from './api';
import { ENTITY_KIND, PRIORITIES, PRIORITY, RECUR, TASK_TYPE, TASK_TYPES } from './labels';
import { DEFAULT_SLA, durationLabel } from './sla';
import { PersonSelect, RelatedLinks, SwitchRow, usePeople } from './ui';
import type { EntityKind, EntityRef, Priority, RecurFreq, TaskInput, TaskType, WorkTask } from './types';

/** datetime-local 값 ↔ ISO (브라우저 현지 시간) */
export const toLocalInput = (iso: string | null | undefined) => {
  if (!iso) return '';
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return '';
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
};
export const fromLocalInput = (v: string) => (v ? new Date(v).toISOString() : null);

const PICK_KINDS: EntityKind[] = ['release', 'contact', 'dsp', 'inquiry', 'document', 'org', 'payout'];

/** 연결할 항목 고르기 — 목록을 불러올 수 있는 종류는 고르고, 아니면 번호를 직접 넣는다 */
function RelatedPicker({ onAdd }: { onAdd: (r: EntityRef) => void }) {
  const [kind, setKind] = useState<EntityKind>('release');
  const [options, setOptions] = useState<EntityRef[] | null>(null);
  const [value, setValue] = useState('');
  const [manualId, setManualId] = useState('');
  const [manualLabel, setManualLabel] = useState('');

  useEffect(() => {
    let alive = true;
    setOptions(null); setValue('');
    const load: Promise<EntityRef[]> | null =
      kind === 'release' ? staffApi.releases('PENDING').then(r => r.items.map(x => ({ kind, id: x.id, label: x.title })))
        : kind === 'contact' ? workApi.contacts().then(r => r.items.map(x => ({ kind, id: x.id, label: x.name })))
          : kind === 'dsp' ? Promise.resolve(Object.entries(DSP_NAME).map(([id, label]) => ({ kind, id, label })))
            : kind === 'inquiry' ? staffApi.inquiries('OPEN').then(r => r.items.map(x => ({ kind, id: x.id, label: x.subject })))
              : kind === 'document' ? staffApi.documents('REVIEW').then(r => r.items.map(x => ({ kind, id: x.id, label: x.title })))
                : null;
    if (!load) { setOptions([]); return; }
    load.then(v => { if (alive) setOptions(v); }).catch(() => { if (alive) setOptions([]); });
    return () => { alive = false; };
  }, [kind]);

  const add = () => {
    const picked = options?.find(o => o.id === value);
    if (picked) { onAdd(picked); setValue(''); return; }
    if (manualId.trim()) { onAdd({ kind, id: manualId.trim(), label: manualLabel.trim() || manualId.trim() }); setManualId(''); setManualLabel(''); }
  };

  return (
    <div className="adm-w-picker">
      <select className="adm-select" aria-label="연결할 종류" value={kind} onChange={e => setKind(e.target.value as EntityKind)}>
        {PICK_KINDS.map(k => <option key={k} value={k}>{ENTITY_KIND[k].label}</option>)}
      </select>
      {options && options.length > 0 ? (
        <select className="adm-select" aria-label="연결할 항목" value={value} onChange={e => setValue(e.target.value)}>
          <option value="">항목 고르기</option>
          {options.map(o => <option key={o.id} value={o.id}>{o.label}</option>)}
        </select>
      ) : (
        <>
          <input className="adm-input" aria-label="번호" placeholder={options ? '번호 (ID)' : '불러오는 중…'} value={manualId} onChange={e => setManualId(e.target.value)} />
          <input className="adm-input" aria-label="표시 이름" placeholder="표시 이름" value={manualLabel} onChange={e => setManualLabel(e.target.value)} />
        </>
      )}
      <button type="button" className="adm-btn soft small" onClick={add} disabled={!value && !manualId.trim()}><Glyph name="plus" size={14} />연결</button>
    </div>
  );
}

export function TaskForm({ task, initial, onSaved }: { task?: WorkTask; initial?: Partial<TaskInput>; onSaved: (t: WorkTask) => void }) {
  const close = useModalClose();
  const toast = useToast();
  const { me } = useStaff();
  const people = usePeople();
  const src = task ?? initial ?? {};
  const [title, setTitle] = useState(src.title ?? '');
  const [description, setDescription] = useState(src.description ?? '');
  const [type, setType] = useState<TaskType>(src.type ?? 'GENERAL');
  const [priority, setPriority] = useState<Priority>(task?.priority_raised_from ?? src.priority ?? 'NORMAL');
  const [assignee, setAssignee] = useState<string | null>(src.assignee_id !== undefined ? src.assignee_id ?? null : me.user_id);
  const [reviewer, setReviewer] = useState<string | null>(src.reviewer_id ?? null);
  const [due, setDue] = useState(toLocalInput(src.due_at));
  const [recurOn, setRecurOn] = useState(!!src.recurrence);
  const [freq, setFreq] = useState<RecurFreq>(src.recurrence?.freq ?? 'WEEKLY');
  const [interval, setInterval] = useState(src.recurrence?.interval ?? 1);
  const [related, setRelated] = useState<EntityRef[]>(src.related ?? []);
  const [tags, setTags] = useState((src.tags ?? []).join(', '));
  const [busy, setBusy] = useState(false);
  const sla = DEFAULT_SLA.find(p => p.type === type);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busy || !title.trim()) return;
    setBusy(true);
    const body: TaskInput = {
      title: title.trim(), description: description.trim(), type, priority, assignee_id: assignee, reviewer_id: reviewer,
      due_at: fromLocalInput(due), recurrence: recurOn ? { freq, interval: Math.max(1, Math.min(12, interval)) } : null,
      related, tags: tags.split(',').map(t => t.trim()).filter(Boolean).slice(0, 8),
    };
    try {
      const saved = task ? await workApi.updateTask(task.id, { ...body, row_version: task.row_version }) : await workApi.createTask(body);
      toast(task ? '업무를 고쳤어요.' : `${saved.key} 업무를 만들었어요.`, 'success');
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
      <div className="adm-field">
        <label htmlFor="tTitle">제목 <span className="required">*</span></label>
        <input id="tTitle" data-autofocus className="adm-input" maxLength={200} value={title} onChange={e => setTitle(e.target.value)} placeholder="예: Lumi Records 계약 갱신 검토" />
      </div>
      <div className="adm-w-grid2">
        <div className="adm-field">
          <label htmlFor="tType">업무 유형</label>
          <select id="tType" className="adm-select adm-w-select" value={type} onChange={e => setType(e.target.value as TaskType)}>
            {TASK_TYPES.map(t => <option key={t} value={t}>{TASK_TYPE[t]}</option>)}
          </select>
          {sla && <small>SLA {durationLabel(sla.target_minutes, sla.business_hours)} 안에 처리</small>}
        </div>
        <div className="adm-field">
          <label htmlFor="tDue">마감일</label>
          <input id="tDue" type="datetime-local" className="adm-input" value={due} onChange={e => setDue(e.target.value)} />
          <small>비우면 SLA 기한만 적용돼요.</small>
        </div>
      </div>
      <div className="adm-field">
        <span className="adm-field-label">우선순위</span>
        <Seg label="우선순위" value={priority} onChange={setPriority} options={PRIORITIES.map(p => [p, PRIORITY[p][0]] as const)} />
      </div>
      <div className="adm-w-grid2">
        <div className="adm-field">
          <label htmlFor="tAssignee">담당자</label>
          <PersonSelect id="tAssignee" value={assignee} onChange={setAssignee} people={people} empty="담당자 없음" />
        </div>
        <div className="adm-field">
          <label htmlFor="tReviewer">검토자</label>
          <PersonSelect id="tReviewer" value={reviewer} onChange={setReviewer} people={people} empty="검토 없이 완료" />
          <small>검토자가 있으면 완료 전에 승인을 받아요.</small>
        </div>
      </div>
      <div className="adm-field">
        <label htmlFor="tDesc">내용</label>
        <textarea id="tDesc" className="adm-textarea" rows={4} maxLength={8000} value={description} onChange={e => setDescription(e.target.value)} placeholder="무엇을, 어디까지 하면 끝나는지 적어 주세요." />
      </div>
      <div className="adm-field">
        <span className="adm-field-label">연결 항목</span>
        <RelatedLinks items={related} onRemove={i => setRelated(related.filter((_, j) => j !== i))} />
        <RelatedPicker onAdd={r => setRelated(list => (list.some(x => x.kind === r.kind && x.id === r.id) ? list : [...list, r]))} />
      </div>
      <div className="adm-w-box">
        <SwitchRow title="반복 업무" hint={recurOn ? '완료하면 다음 업무가 자동으로 만들어져요.' : '정기 점검·월말 정산 같은 일에 켜 주세요.'} checked={recurOn} onChange={setRecurOn} />
        {recurOn && (
          <div className="adm-w-recur">
            <Seg label="반복 주기" value={freq} onChange={setFreq} options={(['DAILY', 'WEEKLY', 'MONTHLY'] as const).map(f => [f, RECUR[f][0]] as const)} />
            <label className="adm-w-inline">
              <input type="number" className="adm-input" min={1} max={12} value={interval} onChange={e => setInterval(Number(e.target.value) || 1)} aria-label="반복 간격" />
              <span>{RECUR[freq][1]}마다</span>
            </label>
          </div>
        )}
      </div>
      <div className="adm-field" style={{ marginTop: 16 }}>
        <label htmlFor="tTags">태그</label>
        <input id="tTags" className="adm-input" value={tags} onChange={e => setTags(e.target.value)} placeholder="쉼표로 구분 (예: 월말, 긴급)" />
      </div>
      <div className="adm-form-actions">
        <button type="button" className="adm-btn soft" onClick={close}>취소</button>
        <button type="submit" className="adm-btn primary" disabled={busy || !title.trim()}>{busy ? '저장 중…' : task ? '저장' : '업무 만들기'}</button>
      </div>
    </form>
  );
}
