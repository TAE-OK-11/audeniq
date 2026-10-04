// 업무 API — 백엔드 `/api/staff/work/*`. 관리자 API(staffApi)와 같은 세션·CSRF를 쓰고,
// 체험(목) 빌드에서는 브라우저 메모리의 예시 데이터(work/mock.ts)로 같은 화면을 확인한다.
import { req } from '../../api/http';
import { MOCK } from '../../lib/mode';
import type {
  Analytics, Contact, ContactActivity, ContactDetail, ContactInput, ContactKind, DspOps, DspOpsPatch,
  ExceptionAction, ExceptionKind, ExceptionStatus, InboxItem, InboxKind, NoteInput, SlaPolicy, StaffPerson,
  TaskDetail, TaskInput, TaskPatch, TaskType, WorkException, WorkNote, WorkSummary, WorkTask, EntityKind, ActivityKind,
} from './types';

interface Page<T> { items: T[] }
const qs = (o: Record<string, string | number | boolean | undefined | null>) => {
  const q = new URLSearchParams();
  for (const [k, v] of Object.entries(o)) if (v != null && v !== '' && v !== false) q.set(k, String(v));
  const s = q.toString();
  return s ? `?${s}` : '';
};
const id = (v: string) => encodeURIComponent(v);

export interface TaskQuery {
  /** mine = 내가 담당 · review = 내가 검토자 · created = 내가 만든 · all */
  view?: 'mine' | 'review' | 'created' | 'all';
  status?: 'open' | 'done' | 'all';
  type?: TaskType | '';
  assignee?: string;
  q?: string;
  related_kind?: EntityKind;
  related_id?: string;
}

const B = '/api/staff/work';

const remote = {
  summary: () => req<WorkSummary>(`${B}/summary`),
  people: () => req<Page<StaffPerson>>(`${B}/people`),

  inbox: (kind?: InboxKind | '', unread?: boolean) => req<Page<InboxItem>>(`${B}/inbox${qs({ kind, unread })}`),
  readInbox: (ids: string[]) => req<{ ok: true }>(`${B}/inbox/read`, { method: 'POST', body: { ids } }),

  tasks: (q: TaskQuery = {}) => req<Page<WorkTask>>(`${B}/tasks${qs({ ...q })}`),
  task: (taskId: string) => req<TaskDetail>(`${B}/tasks/${id(taskId)}`),
  createTask: (body: TaskInput) => req<WorkTask>(`${B}/tasks`, { method: 'POST', body }),
  updateTask: (taskId: string, body: TaskPatch) => req<WorkTask>(`${B}/tasks/${id(taskId)}`, { method: 'POST', body }),

  notes: (kind: EntityKind, targetId: string) => req<Page<WorkNote>>(`${B}/notes${qs({ target_kind: kind, target_id: targetId })}`),
  addNote: (body: NoteInput) => req<WorkNote>(`${B}/notes`, { method: 'POST', body }),
  resolveNote: (noteId: string, resolved: boolean) => req<WorkNote>(`${B}/notes/${id(noteId)}/resolve`, { method: 'POST', body: { resolved } }),
  attachmentUrl: (attId: string) => `${(import.meta.env.VITE_API_BASE ?? '').replace(/\/$/, '')}${B}/attachments/${id(attId)}`,

  exceptions: (kind?: ExceptionKind | '', status?: ExceptionStatus | 'active' | '') => req<Page<WorkException>>(`${B}/exceptions${qs({ kind, status })}`),
  actException: (exId: string, body: ExceptionAction) => req<WorkException>(`${B}/exceptions/${id(exId)}`, { method: 'POST', body }),

  sla: () => req<Page<SlaPolicy>>(`${B}/sla`),
  saveSla: (p: SlaPolicy) => req<SlaPolicy>(`${B}/sla/${id(p.type)}`, { method: 'PUT', body: p }),

  contacts: (kind?: ContactKind | '', q?: string, due?: boolean) => req<Page<Contact>>(`${B}/contacts${qs({ kind, q, due })}`),
  contact: (cId: string) => req<ContactDetail>(`${B}/contacts/${id(cId)}`),
  createContact: (body: ContactInput) => req<Contact>(`${B}/contacts`, { method: 'POST', body }),
  updateContact: (cId: string, body: Partial<ContactInput>) => req<Contact>(`${B}/contacts/${id(cId)}`, { method: 'POST', body }),
  addActivity: (cId: string, body: { kind: ActivityKind; summary: string; next_follow_up_at?: string | null }) =>
    req<ContactActivity>(`${B}/contacts/${id(cId)}/activities`, { method: 'POST', body }),

  dspOps: () => req<Page<DspOps>>(`${B}/dsp-ops`),
  updateDspOps: (code: string, body: DspOpsPatch) => req<DspOps>(`${B}/dsp-ops/${id(code)}`, { method: 'POST', body }),

  analytics: (days: number) => req<Analytics>(`${B}/analytics${qs({ days })}`),
};

export type WorkApi = typeof remote;

function lazyMock(): WorkApi {
  let mod: Promise<WorkApi> | null = null;
  const load = () => (mod ??= import('./mock').then(m => m.mockWork as WorkApi));
  return new Proxy({} as WorkApi, {
    get: (_, key) => {
      // 첨부 주소는 동기 함수 — 체험 모드에서는 내려받기 대신 이름만 보여 준다
      if (key === 'attachmentUrl') return () => '';
      return (...args: unknown[]) => load().then(api => (api[key as keyof WorkApi] as (...a: unknown[]) => unknown)(...args));
    },
  });
}

export const workApi: WorkApi = MOCK ? lazyMock() : remote;
