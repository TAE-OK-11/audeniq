// 업무(Work) 모듈 타입 — 백엔드 `/api/staff/work/*` (crates/core/src/work.rs)와 같은 모양.
// 업무(Task) · 업무함(Inbox) · SLA · 예외 처리 · 내부 메모 · 거래처(CRM Lite) · DSP 운영 · 업무 분석.

export type TaskStatus = 'TODO' | 'IN_PROGRESS' | 'IN_REVIEW' | 'BLOCKED' | 'DONE' | 'CANCELLED';
export type Priority = 'URGENT' | 'HIGH' | 'NORMAL' | 'LOW';
export type TaskType =
  | 'URGENT_DELIVERY' | 'RELEASE' | 'RIGHTS' | 'CONTRACT' | 'DSP_INQUIRY'
  | 'SETTLEMENT' | 'LEGAL' | 'SUPPORT' | 'GENERAL';

/** 다른 모듈과 연결 — 발매, 작업 공간, 거래처, DSP, 문의, 서류, 예외, 지급 */
export type EntityKind = 'release' | 'org' | 'contact' | 'dsp' | 'inquiry' | 'document' | 'exception' | 'payout' | 'task';
export interface EntityRef { kind: EntityKind; id: string; label: string }

export type RecurFreq = 'DAILY' | 'WEEKLY' | 'MONTHLY';
export interface Recurrence {
  freq: RecurFreq;
  /** 매 n일/주/월 */
  interval: number;
  /** 다음 업무가 만들어지는 시각 */
  next_at?: string | null;
}

export type SlaState = 'NONE' | 'OK' | 'DUE_SOON' | 'OVERDUE' | 'MET' | 'MISSED';

export interface WorkTask {
  id: string;
  /** 사람이 부르는 번호 (WRK-1024) */
  key: string;
  title: string;
  description: string;
  type: TaskType;
  status: TaskStatus;
  priority: Priority;
  assignee_id: string | null;
  reviewer_id: string | null;
  due_at: string | null;
  /** 업무 유형 SLA로 계산한 처리 기한 */
  sla_due_at: string | null;
  sla_state: SlaState;
  /** 0 없음 · 1 담당자 알림 · 2 책임자 escalation */
  escalation_level: number;
  /** SLA 때문에 자동으로 올라간 우선순위면 원래 값 */
  priority_raised_from: Priority | null;
  recurrence: Recurrence | null;
  related: EntityRef[];
  tags: string[];
  created_by: string;
  created_at: string;
  updated_at: string;
  /** 담당자가 맡은 시각 — 미처리 시간 계산 */
  assigned_at: string | null;
  started_at: string | null;
  completed_at: string | null;
  note_count: number;
  row_version: number;
}

export interface TaskInput {
  title: string;
  description?: string;
  type: TaskType;
  priority: Priority;
  assignee_id?: string | null;
  reviewer_id?: string | null;
  due_at?: string | null;
  recurrence?: Recurrence | null;
  related?: EntityRef[];
  tags?: string[];
}

export type TaskPatch = Partial<TaskInput> & {
  status?: TaskStatus;
  /** 검토자 결정 — IN_REVIEW에서만 */
  review?: 'APPROVE' | 'REQUEST_CHANGES';
  review_note?: string;
  row_version: number;
};

export interface TaskEvent { at: string; actor_id: string | null; action: string; detail: string | null }
export interface TaskDetail { task: WorkTask; events: TaskEvent[] }

export interface StaffPerson { user_id: string; email: string; name: string; role: string }

// ---------- 업무함 ----------
export type InboxKind =
  | 'ASSIGNED' | 'REVIEW_REQUEST' | 'CORRECTION' | 'MENTION'
  | 'AUTOMATION_FAILED' | 'SLA_BREACH' | 'INQUIRY' | 'RIGHTS';

export interface InboxItem {
  id: string;
  kind: InboxKind;
  title: string;
  summary: string;
  priority: Priority;
  due_at: string | null;
  at: string;
  /** 누르면 갈 관리자 주소 */
  link: string;
  read: boolean;
  ref: EntityRef | null;
}

export interface WorkSummary {
  inbox_unread: number;
  inbox_total: number;
  my_open: number;
  due_soon: number;
  overdue: number;
  exceptions_open: number;
  follow_ups_due: number;
  by_kind: Partial<Record<InboxKind, number>>;
}

// ---------- SLA ----------
export interface SlaPolicy {
  type: TaskType;
  /** 처리 제한 시간(분) */
  target_minutes: number;
  /** 영업시간(평일 09–18시, 한국 시간)만 센다 */
  business_hours: boolean;
  /** 남은 시간이 이 비율 밑이면 Due Soon (0.25 = 25%) */
  warn_ratio: number;
  /** 이 시간(분) 동안 담당자가 시작하지 않으면 담당자에게 알림 */
  idle_alert_minutes: number;
  /** 기한을 넘기면 보낼 대상 역할 */
  escalate_to: 'ADMIN' | 'REVIEWER' | 'OPERATOR' | 'SUPPORT';
  /** 기한이 가까워지면 우선순위를 한 단계 올린다 */
  auto_raise_priority: boolean;
  enabled: boolean;
}

// ---------- 예외 처리 ----------
export type ExceptionKind = 'QC' | 'RIGHTS_DOCS' | 'DSP_REJECTION' | 'METADATA_CONFLICT' | 'SETTLEMENT_MISMATCH' | 'DELIVERY_FAILURE';
export type Severity = 'CRITICAL' | 'HIGH' | 'MEDIUM' | 'LOW';
export type ExceptionStatus = 'OPEN' | 'ASSIGNED' | 'RESOLVED' | 'IGNORED';

export interface WorkException {
  id: string;
  kind: ExceptionKind;
  severity: Severity;
  status: ExceptionStatus;
  title: string;
  detail: string;
  /** 문제를 올린 자동화 (qc-worker, delivery-runner …) */
  source: string;
  related: EntityRef[];
  assignee_id: string | null;
  task_id: string | null;
  occurrences: number;
  first_seen: string;
  last_seen: string;
  resolved_at: string | null;
  resolution: string | null;
}

export type ExceptionAction =
  | { action: 'ASSIGN'; assignee_id: string }
  | { action: 'RESOLVE'; resolution: string }
  | { action: 'IGNORE'; resolution: string }
  | { action: 'REOPEN' }
  | { action: 'TO_TASK'; assignee_id?: string | null; priority?: Priority };

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
  resolved_by: string | null;
  resolved_at: string | null;
  created_at: string;
}
export interface NoteInput {
  target_kind: EntityKind;
  target_id: string;
  body: string;
  mentions: string[];
  /** 먼저 올린 첨부(workApi.upload)의 id — 파일당 2MB · 메모당 3개까지 */
  attachments: string[];
}

// ---------- 거래처 (CRM Lite) ----------
export type ContactKind = 'ARTIST' | 'LABEL' | 'DSP' | 'PARTNER';
export type ContractStage = 'NONE' | 'NEGOTIATING' | 'SENT' | 'SIGNED' | 'RENEWAL' | 'ENDED';
export interface Contact {
  id: string;
  kind: ContactKind;
  name: string;
  company: string;
  person: string;
  email: string;
  phone: string;
  owner_id: string | null;
  last_contact_at: string | null;
  next_follow_up_at: string | null;
  contract_stage: ContractStage;
  tags: string[];
  org_id: string | null;
  dsp_code: string | null;
  memo: string;
  created_at: string;
  updated_at: string;
}
export type ContactInput = Pick<Contact, 'kind' | 'name' | 'company' | 'person' | 'email' | 'phone' | 'owner_id' | 'next_follow_up_at' | 'contract_stage' | 'tags' | 'memo'>;
export type ActivityKind = 'CALL' | 'EMAIL' | 'MEETING' | 'NOTE';
export interface ContactActivity { id: string; kind: ActivityKind; summary: string; by: string; at: string }
export interface ContactDetail { contact: Contact; activities: ContactActivity[]; tasks: WorkTask[] }

// ---------- DSP 운영 ----------
export type IntegrationKind = 'DDEX_SFTP' | 'DDEX_API' | 'API' | 'MANUAL';
export type IntegrationHealth = 'HEALTHY' | 'DEGRADED' | 'DOWN' | 'ONBOARDING';
export interface DspOps {
  code: string;
  name: string;
  region: 'KR' | 'GLOBAL';
  contract_stage: ContractStage;
  contract_renewal_at: string | null;
  integration: IntegrationKind;
  health: IntegrationHealth;
  last_sync_at: string | null;
  owner_id: string | null;
  /** 이 DSP 문의 응답 SLA(시간) */
  sla_hours: number;
  deliveries_30d: number;
  failures_30d: number;
  /** 최근 8주 장애율(%) — 오래된 순 */
  failure_trend: number[];
  avg_approval_hours: number | null;
  rejections: { reason: string; count: number }[];
  open_inquiries: number;
  recent_inquiries: { id: string; subject: string; at: string; status: 'OPEN' | 'ANSWERED' | 'CLOSED' }[];
  sla_breaches_30d: number;
}
export type DspOpsPatch = Partial<Pick<DspOps, 'owner_id' | 'contract_stage' | 'contract_renewal_at' | 'sla_hours' | 'health'>>;

// ---------- 업무 분석 ----------
export interface Analytics {
  days: number;
  daily: { date: string; manual: number; auto: number; created: number }[];
  people: { user_id: string; done: number; avg_minutes: number; open: number; overdue: number }[];
  avg_cycle_minutes: number;
  /** 단계별 체류 시간 */
  stages: { key: string; label: string; avg_hours: number; p90_hours: number; wip: number }[];
  reject_rate: number;
  auto_rate: number;
  sla_met_rate: number;
  dsp_failure: { code: string; name: string; rate: number; failures: number }[];
  by_type: { type: TaskType; count: number }[];
}
