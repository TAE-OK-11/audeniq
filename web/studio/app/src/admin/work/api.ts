// 업무 API — 백엔드 `/api/staff/work/*`. 관리자 API(staffApi)와 같은 세션·CSRF를 쓰고,
// 체험(목) 빌드에서는 브라우저 메모리의 예시 데이터(work/mock.ts)로 같은 화면을 확인한다.
import { putToGrant, req, type UploadGrant } from '../../api/http';
import { MOCK } from '../../lib/mode';
import type {
  EntityKind, EntityRef, NoteInput, NoteRevision, StaffPerson, TaskDetail, TaskInput, TaskPatch, TaskQuery,
  WorkNote, WorkTask, LinkInput, InboxPage, InboxQuery, WorkSummary, ExceptionPage, ExceptionQuery,
  ExceptionActInput, ExceptionItem, ExceptionReportInput, SlaOverview, SlaPolicyInput,
  Channel, ActivityInput, ContactInput, DspOps, RelInput, RelPatch, RelQuery, Relationship, RelationshipDetail,
  Board, BoardInput, BoardQuery, BoardRule, BoardView, ColumnInput, MoveInput, RuleInput, AutoTrigger,
  DocComment, DocCreateInput, DocDetail, DocPermissions, DocSaveInput, DocSaved, DocSearchQuery, DocStatusAction, DocSummary,
  DocTemplate, DocVersionBody, DocVersions, DocGrant, DocAccess, EmbedData,
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

  // 팀 대화
  channels: (archived = false) => req<Page<Channel>>(`${B}/channels${qs({ archived })}`),
  createChannel: (body: { name: string; topic: string }) => req<Channel>(`${B}/channels`, { method: 'POST', body }),
  updateChannel: (chId: string, body: { name: string; topic: string }) => req<Channel>(`${B}/channels/${id(chId)}`, { method: 'POST', body }),
  archiveChannel: (chId: string) => req<Channel>(`${B}/channels/${id(chId)}/archive`, { method: 'POST' }),
  readChannel: (chId: string) => req<{ read: string }>(`${B}/channels/${id(chId)}/read`, { method: 'POST' }),
  /** 최근 메시지부터 (화면에는 오래된 순) — before로 더 이전 */
  messages: (chId: string, before?: string, latest = 60) =>
    req<{ items: WorkNote[]; has_more: boolean }>(`${B}/notes${qs({ target_kind: 'channel', target_id: chId, latest, before })}`),

  // 칸반 보드 — 카드는 업무(work.tasks), 열은 상태와 짝
  boards: () => req<Page<Board>>(`${B}/boards`),
  board: (boardId: string, q: BoardQuery = {}) => req<BoardView>(`${B}/boards/${id(boardId)}${qs({ ...q })}`),
  createBoard: (body: BoardInput) => req<BoardView>(`${B}/boards`, { method: 'POST', body }),
  updateBoard: (boardId: string, body: BoardInput) => req<BoardView>(`${B}/boards/${id(boardId)}`, { method: 'POST', body }),
  archiveBoard: (boardId: string) => req<{ archived: string }>(`${B}/boards/${id(boardId)}/archive`, { method: 'POST' }),
  addColumn: (boardId: string, body: ColumnInput) => req<BoardView>(`${B}/boards/${id(boardId)}/columns`, { method: 'POST', body }),
  updateColumn: (colId: string, body: ColumnInput) => req<BoardView>(`${B}/board-columns/${id(colId)}`, { method: 'POST', body }),
  archiveColumn: (colId: string, moveTo?: string | null) => req<BoardView>(`${B}/board-columns/${id(colId)}/archive`, { method: 'POST', body: { move_to: moveTo ?? null } }),
  orderColumns: (boardId: string, ids: string[]) => req<BoardView>(`${B}/boards/${id(boardId)}/columns/order`, { method: 'POST', body: { ids } }),
  moveCard: (taskId: string, body: MoveInput) => req<{ card: WorkTask; redirected: boolean }>(`${B}/tasks/${id(taskId)}/move`, { method: 'POST', body }),
  rules: () => req<{ items: BoardRule[]; triggers: AutoTrigger[] }>(`${B}/board-rules`),
  saveRule: (body: RuleInput) => req<{ items: BoardRule[]; triggers: AutoTrigger[] }>(`${B}/board-rules`, { method: 'POST', body }),
  runAutomation: () => req<{ created: number; at: string }>(`${B}/automation/run`, { method: 'POST' }),

  // AUDENIQ Docs — 블록 JSON 문서, 서버가 접근 수준·버전 충돌을 판단
  docTree: (parent?: string | null, archived = false) => req<Page<DocSummary>>(`${B}/docs${qs({ parent, archived })}`),
  searchDocs: (q: DocSearchQuery) => req<Page<DocSummary>>(`${B}/docs/search${qs({ ...q })}`),
  doc: (docId: string) => req<DocDetail>(`${B}/docs/${id(docId)}`),
  createDoc: (body: DocCreateInput) => req<DocDetail>(`${B}/docs`, { method: 'POST', body }),
  saveDoc: (docId: string, body: DocSaveInput) => req<DocSaved>(`${B}/docs/${id(docId)}`, { method: 'POST', body }),
  moveDoc: (docId: string, body: { parent_id: string | null; before_id?: string | null; after_id?: string | null }) =>
    req<DocDetail>(`${B}/docs/${id(docId)}/move`, { method: 'POST', body }),
  duplicateDoc: (docId: string) => req<DocDetail>(`${B}/docs/${id(docId)}/duplicate`, { method: 'POST' }),
  archiveDoc: (docId: string) => req<{ archived: string[] }>(`${B}/docs/${id(docId)}/archive`, { method: 'POST' }),
  restoreDoc: (docId: string) => req<DocDetail>(`${B}/docs/${id(docId)}/restore`, { method: 'POST' }),
  deleteDoc: (docId: string) => req<{ deleted: string[] }>(`${B}/docs/${id(docId)}/delete`, { method: 'POST' }),
  docStatus: (docId: string, action: DocStatusAction, reason?: string) => req<DocDetail>(`${B}/docs/${id(docId)}/status`, { method: 'POST', body: { action, reason: reason ?? null } }),
  docVersions: (docId: string) => req<DocVersions>(`${B}/docs/${id(docId)}/versions`),
  docVersion: (docId: string, v: number) => req<DocVersionBody>(`${B}/docs/${id(docId)}/versions/${v}`),
  saveDocVersion: (docId: string, label?: string) => req<DocVersions>(`${B}/docs/${id(docId)}/versions`, { method: 'POST', body: { label: label ?? null } }),
  restoreDocVersion: (docId: string, version: number, baseVersion: number) =>
    req<DocDetail>(`${B}/docs/${id(docId)}/restore-version`, { method: 'POST', body: { version, base_version: baseVersion } }),
  docPermissions: (docId: string) => req<DocPermissions>(`${B}/docs/${id(docId)}/permissions`),
  setDocPermissions: (docId: string, body: { staff_access: DocAccess | null; grants: DocGrant[]; apply_to_children: boolean }) =>
    req<DocPermissions>(`${B}/docs/${id(docId)}/permissions`, { method: 'POST', body }),
  docTemplates: () => req<Page<DocTemplate>>(`${B}/doc-templates`),
  saveDocTemplate: (body: { from_doc: string; name: string; description: string }) => req<Page<DocTemplate>>(`${B}/doc-templates`, { method: 'POST', body }),
  archiveDocTemplate: (tplId: string) => req<Page<DocTemplate>>(`${B}/doc-templates/${id(tplId)}/archive`, { method: 'POST' }),
  /** 문서 댓글 — 메모(target_kind 'doc'), 답글(parent_id)·위치(anchor) */
  docComments: (docId: string) => req<Page<DocComment>>(`${B}/notes${qs({ target_kind: 'doc', target_id: docId })}`),
  addDocComment: (body: { target_id: string; body: string; mentions: string[]; parent_id?: string | null; anchor?: DocComment['anchor'] }) =>
    req<DocComment>(`${B}/notes`, { method: 'POST', body: { target_kind: 'doc', attachments: [], ...body } }),
  /** 문서 속 AUDENIQ 블록의 실시간 값 — 'kind:id[:view]' 여러 개를 한 번에 */
  embeds: (refs: string[]) => req<{ items: Record<string, EmbedData> }>(`${B}/embeds${qs({ refs: refs.join(',') })}`),
  /** 문서에 넣을 파일 — 메모 첨부와 같은 업로드, 문서에 저장되면 문서 권한으로 내려받는다 */
  attachmentInline: (attId: string) => `${(import.meta.env.VITE_API_BASE ?? '').replace(/\/$/, '')}${B}/attachments/${id(attId)}?inline=true`,

  attachmentUrl: (attId: string) => `${(import.meta.env.VITE_API_BASE ?? '').replace(/\/$/, '')}${B}/attachments/${id(attId)}`,
};

export type WorkApi = typeof remote;

function lazyMock(): WorkApi {
  let mod: Promise<WorkApi> | null = null;
  const load = () => (mod ??= import('./mock').then(m => m.mockWork as WorkApi));
  return new Proxy({} as WorkApi, {
    get: (_, key) => {
      // 첨부 주소는 동기 함수 — 체험 모드에서는 내려받기 대신 이름만 보여 준다
      if (key === 'attachmentUrl' || key === 'attachmentInline') return () => '';
      return (...args: unknown[]) => load().then(api => (api[key as keyof WorkApi] as (...a: unknown[]) => unknown)(...args));
    },
  });
}

export const workApi: WorkApi = MOCK ? lazyMock() : remote;
