// 업무 API — 백엔드 `/api/staff/work/*`. 관리자 API(staffApi)와 같은 세션·CSRF를 쓰고,
// 체험(목) 빌드에서는 브라우저 메모리의 예시 데이터(work/mock.ts)로 같은 화면을 확인한다.
import { putToGrant, req, type UploadGrant } from '../../api/http';
import { MOCK } from '../../lib/mode';
import type {
  EntityKind, EntityRef, NoteInput, NoteRevision, StaffPerson, TaskDetail, TaskInput, TaskPatch, TaskQuery,
  WorkNote, WorkTask, LinkInput, InboxPage, InboxQuery, WorkSummary, ExceptionPage, ExceptionQuery,
  ExceptionActInput, ExceptionItem, ExceptionReportInput, SlaOverview, SlaPolicyInput,
  ActivityInput, ContactInput, DspOps, RelInput, RelPatch, RelQuery, Relationship, RelationshipDetail,
} from './types';

interface Page<T> { items: T[] }
const qs = (o: Record<string, string | number | boolean | undefined | null>) => {
  const q = new URLSearchParams();
  for (const [k, v] of Object.entries(o)) if (v != null && v !== '' && v !== false) q.set(k, String(v));
  const s = q.toString();
  return s ? `?${s}` : '';
};
const id = (v: string) => encodeURIComponent(v);
const B = '/api/staff/work';

const remote = {
  people: () => req<Page<StaffPerson>>(`${B}/people`),
  searchEntities: (kind: EntityKind, q: string) => req<Page<EntityRef>>(`${B}/entities/search${qs({ kind, q })}`),

  tasks: (q: TaskQuery = {}) => req<Page<WorkTask>>(`${B}/tasks${qs({ ...q })}`),
  task: (taskId: string) => req<TaskDetail>(`${B}/tasks/${id(taskId)}`),
  createTask: (body: TaskInput) => req<TaskDetail>(`${B}/tasks`, { method: 'POST', body }),
  updateTask: (taskId: string, body: TaskPatch) => req<TaskDetail>(`${B}/tasks/${id(taskId)}`, { method: 'POST', body }),
  archiveTask: (taskId: string, reason = '') => req<{ archived: string[] }>(`${B}/tasks/${id(taskId)}/archive`, { method: 'POST', body: { reason } }),
  linkTask: (taskId: string, body: LinkInput) => req<TaskDetail>(`${B}/tasks/${id(taskId)}/links`, { method: 'POST', body }),
  unlink: (linkId: string) => req<TaskDetail>(`${B}/links/${id(linkId)}/archive`, { method: 'POST' }),

  notes: (kind: EntityKind, targetId: string, archived = false) => req<Page<WorkNote>>(`${B}/notes${qs({ target_kind: kind, target_id: targetId, archived })}`),
  addNote: (body: NoteInput) => req<WorkNote>(`${B}/notes`, { method: 'POST', body }),
  editNote: (noteId: string, body: { body: string; mentions: string[] }) => req<WorkNote>(`${B}/notes/${id(noteId)}`, { method: 'POST', body }),
  noteRevisions: (noteId: string) => req<Page<NoteRevision>>(`${B}/notes/${id(noteId)}/revisions`),
  resolveNote: (noteId: string, resolved: boolean) => req<WorkNote>(`${B}/notes/${id(noteId)}/resolve`, { method: 'POST', body: { resolved } }),
  archiveNote: (noteId: string) => req<{ id: string }>(`${B}/notes/${id(noteId)}/archive`, { method: 'POST' }),
  mentions: (resolved = false) => req<Page<WorkNote>>(`${B}/mentions${qs({ resolved })}`),

  /** 첨부 — 스튜디오 업로드와 같은 경로: 서명된 주소로 저장소에 직접 올리고, 서버가 확인·검사 */
  upload: async (file: File, onProgress?: (ratio: number) => void): Promise<string> => {
    const contentType = file.type || 'application/octet-stream';
    const r = await req<{ id: string; grant: UploadGrant }>(`${B}/attachments`, { method: 'POST', body: { name: file.name, content_type: contentType, size_bytes: file.size } });
    await putToGrant(r.grant, file, onProgress);
    await req(`${B}/attachments/${id(r.id)}/complete`, { method: 'POST' });
    return r.id;
  },
  // Phase 2 — 업무함 · 예외 처리 · SLA
  summary: () => req<WorkSummary>(`${B}/summary`),
  inbox: (q: InboxQuery = {}) => req<InboxPage>(`${B}/inbox${qs({ ...q })}`),
  markRead: (ids: string[]) => req<{ read: number }>(`${B}/inbox/read`, { method: 'POST', body: { ids } }),
  exceptions: (q: ExceptionQuery = {}) => req<ExceptionPage>(`${B}/exceptions${qs({ ...q })}`),
  actException: (source: string, sourceId: string, body: ExceptionActInput) =>
    req<{ item: ExceptionItem | null }>(`${B}/exceptions/${id(source)}/${id(sourceId)}`, { method: 'POST', body }),
  reportException: (body: ExceptionReportInput) => req<{ id: string }>(`${B}/exceptions/reports`, { method: 'POST', body }),
  sla: () => req<SlaOverview>(`${B}/sla`),
  savePolicy: (body: SlaPolicyInput) => req<SlaOverview>(`${B}/sla/policies`, { method: 'POST', body }),
  archivePolicy: (policyId: string) => req<SlaOverview>(`${B}/sla/policies/${id(policyId)}/archive`, { method: 'POST' }),
  saveCalendar: (calId: string, body: { name: string; work_start: string; work_end: string; workdays: number[] }) =>
    req<SlaOverview>(`${B}/calendars/${id(calId)}`, { method: 'POST', body }),
  setHoliday: (calId: string, day: string, name?: string) =>
    req<SlaOverview>(`${B}/calendars/${id(calId)}/holidays`, { method: 'POST', body: { day, name: name ?? null } }),

  // Phase 3 — CRM Lite · DSP 운영
  relationships: (q: RelQuery = {}) => req<{ items: Relationship[]; counts: Record<string, number> }>(`${B}/relationships${qs({ ...q })}`),
  relationship: (relId: string) => req<RelationshipDetail>(`${B}/relationships/${id(relId)}`),
  createRelationship: (body: RelInput) => req<RelationshipDetail>(`${B}/relationships`, { method: 'POST', body }),
  updateRelationship: (relId: string, body: RelPatch) => req<RelationshipDetail>(`${B}/relationships/${id(relId)}`, { method: 'POST', body }),
  archiveRelationship: (relId: string) => req<{ archived: string }>(`${B}/relationships/${id(relId)}/archive`, { method: 'POST' }),
  addContact: (relId: string, body: ContactInput) => req<RelationshipDetail>(`${B}/relationships/${id(relId)}/contacts`, { method: 'POST', body }),
  editContact: (contactId: string, body: ContactInput) => req<RelationshipDetail>(`${B}/relationship-contacts/${id(contactId)}`, { method: 'POST', body }),
  logActivity: (relId: string, body: ActivityInput) => req<RelationshipDetail>(`${B}/relationships/${id(relId)}/activities`, { method: 'POST', body }),
  linkRelationship: (relId: string, body: LinkInput) => req<RelationshipDetail>(`${B}/relationships/${id(relId)}/links`, { method: 'POST', body }),
  unlinkRelationship: (relId: string, linkId: string) => req<RelationshipDetail>(`${B}/relationships/${id(relId)}/links/${id(linkId)}/archive`, { method: 'POST' }),
  dspOps: (dsp?: string) => req<DspOps>(`${B}/dsp-ops${qs({ dsp })}`),

  attachmentUrl: (attId: string) => `${(import.meta.env.VITE_API_BASE ?? '').replace(/\/$/, '')}${B}/attachments/${id(attId)}`,
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
