// 업무(Work) 모듈 타입 — 백엔드 `/api/staff/work/*` (crates/core/src/work)와 같은 모양.
// Phase 1: 업무(Task)·하위 업무·공통 Entity 연결·내부 메모·멘션·첨부. 설계: docs/WORK_MANAGEMENT_PLAN.md

export type TaskStatus = 'TODO' | 'IN_PROGRESS' | 'IN_REVIEW' | 'BLOCKED' | 'DONE' | 'CANCELLED';
export type Priority = 'URGENT' | 'HIGH' | 'NORMAL' | 'LOW';
export type WorkType =
  | 'GENERAL' | 'URGENT_DELIVERY' | 'RELEASE' | 'RIGHTS' | 'CONTRACT' | 'ARTIST'
  | 'DSP_INQUIRY' | 'SETTLEMENT' | 'SUPPORT' | 'LEGAL';

/** 업무·메모가 가리킬 수 있는 AUDENIQ Entity. 이름은 서버가 원천 테이블에서 읽어 준다(저장하지 않음). */
export type EntityKind =
  | 'release' | 'org' | 'artist' | 'label' | 'dsp' | 'inquiry' | 'document' | 'payout' | 'settlement_report' | 'task';

export interface EntityRef {
  kind: EntityKind;
  id: string;
  /** 원천에서 읽은 표시 이름 — 원천이 없어졌으면 null + missing */
  label: string | null;
  sub: string | null;
  missing?: boolean;
  /** 업무 연결이면 연결 id (해제할 때) */
  link_id?: string;
}

export type RecurFreq = 'DAILY' | 'WEEKLY' | 'MONTHLY';
export interface Recurrence { freq: RecurFreq; interval: number; next_at?: string | null }

export interface WorkTask {
  id: string;
  key: string;
  parent_id: string | null;
  title: string;
  description: string;
  type: WorkType;
  status: TaskStatus;
  priority: Priority;
  assignee_id: string | null;
  reviewer_id: string | null;
  due_at: string | null;
  overdue: boolean;
  changes_requested: boolean;
  recurrence: Recurrence | null;
  recurred_from: string | null;
  tags: string[];
  created_by: string;
  created_at: string;
  updated_at: string;
  assigned_at: string | null;
  started_at: string | null;
  completed_at: string | null;
  archived_at: string | null;
  row_version: number;
  subtasks: { total: number; done: number };
  note_count: number;
  links: EntityRef[];
}

export interface TaskEvent { at: string; actor_id: string | null; action: string; detail: string | null }
export interface TaskDetail {
  task: WorkTask;
  parent: { id: string; key: string; title: string; status: TaskStatus } | null;
  subtasks: WorkTask[];
  events: TaskEvent[];
}

export interface LinkInput { entity_kind: EntityKind; entity_id: string }

export interface TaskInput {
  title: string;
  description?: string;
  type: WorkType;
  priority: Priority;
  parent_id?: string | null;
  assignee_id?: string | null;
  reviewer_id?: string | null;
  due_at?: string | null;
  recurrence?: Recurrence | null;
  links?: LinkInput[];
  tags?: string[];
}

export interface TaskPatch {
  row_version: number;
  title?: string;
  description?: string;
  type?: WorkType;
  priority?: Priority;
  assignee_id?: string | null;
  reviewer_id?: string | null;
  due_at?: string | null;
  recurrence?: Recurrence | null;
  tags?: string[];
  status?: TaskStatus;
  review?: 'APPROVE' | 'REQUEST_CHANGES';
  review_note?: string;
}

export interface TaskQuery {
  view?: 'mine' | 'review' | 'created' | 'all';
  status?: 'open' | 'done' | 'all' | 'archived';
  type?: WorkType | '';
  priority?: Priority | '';
  assignee?: string;
  due?: 'overdue' | 'today' | 'week' | '';
  entity_kind?: EntityKind | '';
  entity_id?: string;
  q?: string;
  subtasks?: boolean;
}

export interface StaffPerson { user_id: string; email: string; name: string; role: string }

// ---------- 내부 메모 ----------
export interface NoteAttachment { id: string; name: string; size: number; content_type: string }
export interface WorkNote {
  id: string;
  target_kind: EntityKind;
  target_id: string;
  author_id: string;
  body: string;
  mentions: string[];
  attachments: NoteAttachment[];
  resolved: boolean;
  resolved_at: string | null;
  resolved_by: string | null;
  edited_at: string | null;
  revisions: number;
  created_at: string;
  archived_at: string | null;
  /** 멘션 목록에서만 — 메모가 달린 Entity */
  target?: EntityRef;
}
export interface NoteRevision { body: string; mentions: string[]; edited_by: string; edited_at: string }
export interface NoteInput { target_kind: EntityKind; target_id: string; body: string; mentions: string[]; attachments: string[] }

