// 업무(Work) 모듈 타입 — 백엔드 `/api/staff/work/*` (crates/core/src/work)와 같은 모양.
// Phase 1: 업무(Task)·하위 업무·공통 Entity 연결·내부 메모·멘션·첨부. 설계: docs/WORK_MANAGEMENT_PLAN.md

export type TaskStatus = 'TODO' | 'IN_PROGRESS' | 'IN_REVIEW' | 'BLOCKED' | 'DONE' | 'CANCELLED';
export type Priority = 'URGENT' | 'HIGH' | 'NORMAL' | 'LOW';
export type WorkType =
  | 'GENERAL' | 'URGENT_DELIVERY' | 'RELEASE' | 'RIGHTS' | 'CONTRACT' | 'ARTIST'
  | 'DSP_INQUIRY' | 'SETTLEMENT' | 'SUPPORT' | 'LEGAL';

/** 업무·메모가 가리킬 수 있는 AUDENIQ Entity. 이름은 서버가 원천 테이블에서 읽어 준다(저장하지 않음). */
export type EntityKind =
  | 'release' | 'org' | 'artist' | 'label' | 'dsp' | 'inquiry' | 'document' | 'payout' | 'settlement_report' | 'task' | 'relationship';

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
  /** SLA (Phase 2): 정책에서 정한 기한, 마감일과 둘 중 이른 쪽, 상태, 상향 단계 */
  sla_due_at?: string | null;
  deadline?: string | null;
  sla_state?: SlaState | null;
  escalation_level?: number;
  priority_raised?: Priority | null;
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


// ---------- Phase 2: SLA · 업무함 · 예외 처리 ----------
export type SlaState = 'OK' | 'WARNING' | 'BREACHED';
export type QueueType = 'RELEASE_REVIEW' | 'SECOND_APPROVAL' | 'DOCUMENT_REVIEW' | 'SUPPORT_INQUIRY' | 'EXCEPTION';
export type StaffRoleName = 'ADMIN' | 'REVIEWER' | 'OPERATOR' | 'SUPPORT';

export interface SlaPolicy {
  id: string;
  type: WorkType | QueueType;
  scope_kind: 'dsp' | null;
  scope_id: string | null;
  target_minutes: number;
  calendar_id: string | null;
  warn_ratio: number;
  idle_alert_minutes: number;
  escalate_to: StaffRoleName;
  auto_raise_priority: boolean;
  enabled: boolean;
  updated_at: string;
}
export type SlaPolicyInput = Omit<SlaPolicy, 'id' | 'updated_at'> & { id?: string };
export interface WorkCalendar {
  id: string; name: string; utc_offset_minutes: number; work_start: string; work_end: string; workdays: number[]; holidays: string[];
}
export interface QueueDeadline {
  queue: QueueType; id: string; title: string; org_name: string | null; start: string; due_at: string; state: SlaState;
  escalate_to: StaffRoleName; link: string; owner_id: string | null;
}
export interface SlaOverview {
  policies: SlaPolicy[];
  calendars: WorkCalendar[];
  holiday_names: Record<string, string>;
  board: { queue: QueueType; open: number; warning: number; breached: number }[];
  queue: QueueDeadline[];
  tasks: { id: string; key: string; title: string; type: WorkType; priority: Priority; assignee_id: string | null; deadline: string | null; state: SlaState | null; escalation_level: number }[];
}

export type InboxCategory =
  | 'ASSIGNED' | 'REVIEW_REQUEST' | 'APPROVAL_REQUEST' | 'MENTION' | 'FAILED_AUTOMATION' | 'SLA_WARNING'
  | 'OVERDUE' | 'SUPPORT_ESCALATION' | 'RIGHTS_ISSUE' | 'DISTRIBUTION_FAILURE' | 'EXCEPTION' | 'CONTACT_DUE';
export interface InboxItem {
  id: string;
  category: InboxCategory;
  title: string;
  subtitle: string;
  type: string;
  priority: Priority;
  due_at: string | null;
  sla_state: SlaState | null;
  at: string;
  link: string;
  entity: { kind: EntityKind; id: string } | null;
  flags: string[];
  read: boolean;
}
export interface InboxQuery {
  category?: InboxCategory | '';
  type?: string;
  priority?: Priority | '';
  unread?: boolean;
  due?: 'overdue' | 'today' | 'week' | '';
  entity_kind?: EntityKind | '';
}
export interface InboxPage {
  items: InboxItem[];
  counts: Partial<Record<InboxCategory, { total: number; unread: number }>>;
  total: number;
  unread: number;
}
export interface WorkSummary {
  inbox: { total: number; unread: number };
  overdue: number;
  sla_warning: number;
  exceptions: { open: number; mine: number; critical: number };
}

export type ExceptionKind =
  | 'QC_FAILURE' | 'METADATA_CONFLICT' | 'MISSING_RIGHTS_DOCUMENT' | 'DSP_REJECTION' | 'DISTRIBUTION_FAILURE'
  | 'SETTLEMENT_MISMATCH' | 'AUTOMATION_FAILURE';
export type Severity = 'CRITICAL' | 'HIGH' | 'MEDIUM' | 'LOW';
export type ExceptionSource = 'check' | 'staging' | 'document' | 'rights_hold' | 'reconciliation' | 'delivery_job' | 'report' | 'job' | 'manual';
export interface ExceptionTriage {
  assignee_id: string | null;
  status: 'ACKNOWLEDGED' | 'RESOLVED' | 'IGNORED' | null;
  resolution: string | null;
  resolved_at: string | null;
  resolved_by: string | null;
  retry_count: number;
  last_retry_at: string | null;
  task_id: string | null;
  updated_at: string;
}
export interface ExceptionItem {
  id: string;
  source: ExceptionSource;
  source_id: string;
  kind: ExceptionKind | null;
  severity?: Severity;
  title: string | null;
  detail?: string;
  org_id?: string | null;
  org_name?: string | null;
  dsp?: string | null;
  dsp_name?: string | null;
  entity?: { kind: EntityKind; id: string } | null;
  package_id?: string | null;
  detected_at?: string;
  triage: ExceptionTriage | null;
  closed?: boolean;
  retryable?: boolean;
  can_act?: boolean;
  sla?: { due_at: string; state: SlaState } | null;
}
export interface ExceptionQuery {
  view?: 'open' | 'mine' | 'unassigned' | 'resolved' | 'all';
  kind?: ExceptionKind | '';
  severity?: Severity | '';
  source?: ExceptionSource | '';
  dsp?: string;
  q?: string;
}
export interface ExceptionPage {
  items: ExceptionItem[];
  counts: Partial<Record<ExceptionKind, number>>;
  total?: number;
  mine?: number;
}
export type ExceptionAction = 'ASSIGN' | 'ACK' | 'RESOLVE' | 'IGNORE' | 'REOPEN' | 'RETRY' | 'TO_TASK';
export interface ExceptionActInput {
  action: ExceptionAction;
  assignee_id?: string | null;
  resolution?: string;
  title?: string;
  priority?: Priority;
}
export interface ExceptionReportInput {
  kind: ExceptionKind; severity: Severity; title: string; detail: string; entity_kind?: EntityKind | null; entity_id?: string | null;
}

// ---------- Phase 3: CRM Lite · DSP 운영 ----------
export type RelKind = 'ARTIST' | 'LABEL' | 'DSP' | 'PARTNER' | 'ORG';
export type RelStatus = 'PROSPECT' | 'ACTIVE' | 'AT_RISK' | 'INACTIVE';
export type ContractStatus = 'NONE' | 'NEGOTIATING' | 'SIGNED' | 'EXPIRED' | 'TERMINATED';
export interface RelContract {
  status: ContractStatus;
  /** agreements: 작업 공간의 배급 계약서 · dsp_contract: DSP 계약 경로 · manual: 원천이 없어 직접 관리 */
  source: 'agreements' | 'dsp_contract' | 'manual';
  signed?: number; pending?: number; updated_at?: string | null; route?: string | null; onboarding_stage?: string | null;
}
export interface RelContact { id: string; name: string; role: string; email: string | null; phone: string | null; is_primary?: boolean }
export interface Relationship {
  id: string;
  kind: RelKind;
  entity: { kind: 'artist' | 'label' | 'dsp' | 'org'; id: string } | null;
  name: string | null;
  own_name: string | null;
  missing: boolean;
  owner_id: string | null;
  status: RelStatus;
  next_contact_at: string | null;
  contact_due: boolean;
  last_contact_at: string | null;
  tags: string[];
  contract: RelContract;
  open_tasks: number;
  note_count: number;
  primary_contact: RelContact | null;
  org_id: string | null;
  created_by: string;
  created_at: string;
  updated_at: string;
  row_version: number;
  archived_at: string | null;
}
export type ActivityKind = 'CALL' | 'EMAIL' | 'MEETING' | 'NOTE';
export interface RelActivity { id: string; kind: ActivityKind; summary: string; at: string; by_user: string; contact: string | null }
export interface RelationshipDetail {
  relationship: Relationship;
  contacts: RelContact[];
  activities: RelActivity[];
  links: EntityRef[];
  releases: { id: string; title: string; status: string; created_at: string }[];
  agreements: { id: string; title: string; status: string; release_title: string | null; updated_at: string }[];
}
export interface RelQuery { kind?: RelKind | ''; status?: RelStatus | ''; owner?: string; contact?: 'due' | 'week' | ''; q?: string; archived?: boolean }
export interface RelInput {
  kind: RelKind; entity_kind?: string | null; entity_id?: string | null; name?: string | null; owner_id?: string | null;
  status?: RelStatus; next_contact_at?: string | null; manual_contract_status?: ContractStatus | null; tags?: string[];
}
export interface RelPatch {
  row_version: number; owner_id?: string | null; status?: RelStatus; next_contact_at?: string | null; tags?: string[];
  name?: string; manual_contract_status?: ContractStatus;
}
export interface ContactInput { name: string; role: string; email?: string | null; phone?: string | null; is_primary: boolean; archived?: boolean }
export interface ActivityInput { kind: ActivityKind; summary: string; at?: string | null; contact_id?: string | null; next_contact_at?: string | null }

export interface DspOpsItem {
  code: string;
  name: string;
  route: { transport: string; delivery_enabled: boolean; onboarding_stage: string | null; onboarding_gaps: string[] } | null;
  contract_route: { route: string; contract_live: boolean } | null;
  metrics: {
    total: number; delivered: number; failed: number; in_flight: number; failure_rate: number; rejected: number;
    avg_minutes?: number | null; last_delivered_at?: string | null; last_failure_at?: string | null;
  };
  staging: { pending: number; blocked: number };
  exceptions: { open: number; critical: number; high: number };
  relationship: Relationship | null;
  sla: { type: string; target_minutes: number; calendar_id: string | null }[];
  recent_failures?: { job_id: string; status: string; attempts: number; last_error: string | null; updated_at: string; release_id: string | null; title: string | null; org_name: string }[];
  open_exceptions?: ExceptionItem[];
  [k: string]: unknown;
}
export interface DspOps { items: DspOpsItem[]; window_days: number; live_transmission: boolean }
