// 업무 모듈 표기 — 상태·우선순위·유형을 한국어 라벨과 칩 색(Tone)으로. 관리자 화면의 labels.ts와 같은 방식.
import type { Tone } from '../labels';
import type {
  ActivityKind, ContractStatus, RelKind, RelStatus, EntityKind, ExceptionKind, ExceptionSource, InboxCategory, Priority, QueueType, RecurFreq, Severity, SlaState, StaffRoleName, TaskStatus, WorkType,
} from './types';

export const TASK_STATUS: Record<TaskStatus, [string, Tone]> = {
  TODO: ['할 일', 'gray'],
  IN_PROGRESS: ['진행 중', 'blue'],
  IN_REVIEW: ['검토 중', 'violet'],
  BLOCKED: ['막힘', 'red'],
  DONE: ['완료', 'green'],
  CANCELLED: ['취소', 'gray'],
};
export const OPEN_STATUSES: TaskStatus[] = ['TODO', 'IN_PROGRESS', 'IN_REVIEW', 'BLOCKED'];

export const PRIORITY: Record<Priority, [string, Tone]> = {
  URGENT: ['긴급', 'red'], HIGH: ['높음', 'amber'], NORMAL: ['보통', 'blue'], LOW: ['낮음', 'gray'],
};
export const PRIORITIES: Priority[] = ['URGENT', 'HIGH', 'NORMAL', 'LOW'];

export const WORK_TYPE: Record<WorkType, string> = {
  GENERAL: '일반 업무',
  URGENT_DELIVERY: '긴급 송출',
  RELEASE: '발매 업무',
  RIGHTS: '권리 검토',
  CONTRACT: '계약 검토',
  ARTIST: '아티스트 관리',
  DSP_INQUIRY: 'DSP 문의',
  SETTLEMENT: '정산 확인',
  SUPPORT: '고객 문의',
  LEGAL: '법무 대응',
};
export const WORK_TYPES = Object.keys(WORK_TYPE) as WorkType[];

export const RECUR: Record<RecurFreq, [string, string]> = {
  DAILY: ['매일', '일'], WEEKLY: ['매주', '주'], MONTHLY: ['매월', '개월'],
};
export const recurLabel = (r: { freq: RecurFreq; interval: number }) =>
  r.interval <= 1 ? RECUR[r.freq][0] : `${r.interval}${RECUR[r.freq][1]}마다`;

export const ENTITY_KIND: Record<EntityKind, { label: string; icon: string }> = {
  release: { label: '발매', icon: 'release' },
  org: { label: '작업 공간', icon: 'user' },
  artist: { label: '아티스트', icon: 'music' },
  label: { label: '레이블', icon: 'users' },
  dsp: { label: 'DSP', icon: 'globe' },
  inquiry: { label: '문의', icon: 'inquiry' },
  document: { label: '계약·서류', icon: 'doc' },
  payout: { label: '지급', icon: 'card' },
  settlement_report: { label: '정산 리포트', icon: 'won' },
  task: { label: '업무', icon: 'task' },
  relationship: { label: '관계(CRM)', icon: 'users' },
  channel: { label: '팀 대화', icon: 'inquiry' },
  doc: { label: '문서', icon: 'doc' },
  track: { label: '트랙', icon: 'music' },
  approval: { label: '2차 승인', icon: 'approval' },
};
/** 연결할 때 고르는 순서 */
export const LINK_KINDS: EntityKind[] = ['release', 'track', 'artist', 'label', 'org', 'document', 'dsp', 'inquiry', 'approval', 'settlement_report', 'payout', 'task', 'doc', 'relationship'];

/** Entity가 열리는 관리자 주소 (화면이 없는 종류는 null) */
export function entityLink(kind: EntityKind, id: string): string | null {
  switch (kind) {
    case 'release': return `/admin/reviews/${id}`;
    case 'inquiry': return `/admin/inquiries/${id}`;
    case 'dsp': return '/admin/dsps';
    case 'document': return '/admin/documents';
    case 'payout': return '/admin/payouts';
    case 'task': return `/admin/tasks/${id}`;
    case 'relationship': return `/admin/crm/${id}`;
    case 'channel': return `/admin/chat/${id}`;
    case 'doc': return `/admin/docs/${id}`;
    case 'approval': return '/admin/approvals';
    default: return null;
  }
}

export const DOC_KIND_LABEL: Record<string, string> = { AGREEMENT: '배급 계약서', RIGHTS_PROOF: '권리 증빙' };

export const EVENT_LABEL: Record<string, string> = {
  CREATED: '업무를 만들었어요', ASSIGNED: '담당자를 정했어요', REVIEWER: '검토자를 정했어요', STATUS: '상태를 바꿨어요',
  PRIORITY: '우선순위를 바꿨어요', TYPE: '업무 유형을 바꿨어요', DUE: '마감일을 바꿨어요', RECURRENCE: '반복 설정을 바꿨어요',
  REVIEW_REQUESTED: '검토를 요청했어요', APPROVED: '검토를 승인했어요', CHANGES_REQUESTED: '보완을 요청했어요',
  RECURRED: '반복 업무로 다음 건이 만들어졌어요', EDITED: '내용을 고쳤어요', NOTE: '메모를 남겼어요',
  SLA_ESCALATED: 'SLA를 넘겨 상향됐어요', MOVED: '열을 옮겼어요', LINKED: '연결했어요', UNLINKED: '연결을 해제했어요', SUBTASK_ADDED: '하위 업무를 추가했어요', ARCHIVED: '보관했어요',
};

/** 사람이 읽는 남은 시간 — '2시간 남음', '35분 지남' */
export function remaining(iso: string | null, now = Date.now()): string {
  if (!iso) return '';
  const ms = Date.parse(iso) - now;
  const m = Math.round(Math.abs(ms) / 60_000);
  const text = m < 60 ? `${m}분` : m < 48 * 60 ? `${Math.round(m / 60)}시간` : `${Math.round(m / 1440)}일`;
  return ms < 0 ? `${text} 지남` : `${text} 남음`;
}

// ---------- Phase 2: SLA · 업무함 · 예외 처리 ----------
export const SLA_STATE: Record<SlaState, [string, Tone]> = {
  OK: ['여유', 'green'], WARNING: ['임박', 'amber'], BREACHED: ['초과', 'red'],
};
export const QUEUE_TYPE: Record<QueueType, string> = {
  RELEASE_REVIEW: '발매 심사', SECOND_APPROVAL: '2차 승인', DOCUMENT_REVIEW: '서류 검토', SUPPORT_INQUIRY: '고객 문의', EXCEPTION: '예외 처리',
};
export const QUEUE_TYPES = Object.keys(QUEUE_TYPE) as QueueType[];
/** SLA 정책 대상 이름 — 업무 유형 또는 기존 대기열 */
export const slaTypeLabel = (t: string) => (WORK_TYPE as Record<string, string>)[t] ?? (QUEUE_TYPE as Record<string, string>)[t] ?? t;
export const ROLE_NAME: Record<StaffRoleName, string> = { ADMIN: '관리자', REVIEWER: '심사 담당', OPERATOR: '배급 운영', SUPPORT: '고객 지원' };

export const INBOX_CATEGORY: Record<InboxCategory, { label: string; icon: string; tone: Tone }> = {
  ASSIGNED: { label: '배정된 업무', icon: 'task', tone: 'blue' },
  REVIEW_REQUEST: { label: '검토 요청', icon: 'review', tone: 'violet' },
  APPROVAL_REQUEST: { label: '승인 요청', icon: 'approval', tone: 'violet' },
  MENTION: { label: '멘션', icon: 'at', tone: 'blue' },
  FAILED_AUTOMATION: { label: '자동화 실패', icon: 'warning', tone: 'red' },
  SLA_WARNING: { label: 'SLA 임박', icon: 'clock', tone: 'amber' },
  OVERDUE: { label: '기한 초과', icon: 'clock', tone: 'red' },
  SUPPORT_ESCALATION: { label: '문의 지연', icon: 'inquiry', tone: 'red' },
  RIGHTS_ISSUE: { label: '권리 문제', icon: 'doc', tone: 'amber' },
  DISTRIBUTION_FAILURE: { label: '배급 실패', icon: 'truck', tone: 'red' },
  EXCEPTION: { label: '기타 예외', icon: 'alert', tone: 'amber' },
  CONTACT_DUE: { label: '연락 예정', icon: 'users', tone: 'blue' },
};
export const INBOX_CATEGORIES = Object.keys(INBOX_CATEGORY) as InboxCategory[];
export const INBOX_FLAG: Record<string, string> = { ESCALATED: '상향됨', IDLE: '오래 멈춤', SLA: 'SLA', ASSIGNED: '내 담당' };

export const EXCEPTION_KIND: Record<ExceptionKind, { label: string; icon: string }> = {
  QC_FAILURE: { label: 'QC 실패', icon: 'review' },
  METADATA_CONFLICT: { label: '메타데이터 충돌', icon: 'warning' },
  MISSING_RIGHTS_DOCUMENT: { label: '권리 서류 누락', icon: 'doc' },
  DSP_REJECTION: { label: 'DSP 반려', icon: 'globe' },
  DISTRIBUTION_FAILURE: { label: '배급 실패', icon: 'truck' },
  SETTLEMENT_MISMATCH: { label: '정산 불일치', icon: 'won' },
  AUTOMATION_FAILURE: { label: '자동화 실패', icon: 'alert' },
};
export const EXCEPTION_KINDS = Object.keys(EXCEPTION_KIND) as ExceptionKind[];
export const SEVERITY: Record<Severity, [string, Tone]> = {
  CRITICAL: ['치명', 'red'], HIGH: ['높음', 'amber'], MEDIUM: ['보통', 'blue'], LOW: ['낮음', 'gray'],
};
export const SEVERITIES = Object.keys(SEVERITY) as Severity[];
export const EXCEPTION_SOURCE: Record<ExceptionSource, string> = {
  check: '자동 검사', staging: '배급 준비', document: '권리 서류', rights_hold: '권리 보류', reconciliation: '전송 대조',
  delivery_job: '전송 작업', report: '정산 리포트', job: '자동화 작업', manual: '직접 보고',
};
export const TRIAGE_STATUS: Record<string, [string, Tone]> = {
  ACKNOWLEDGED: ['확인함', 'blue'], RESOLVED: ['해결', 'green'], IGNORED: ['무시', 'gray'],
};

/** 분 → '2시간', '1영업일 3시간' 같은 사람이 읽는 길이 (영업일은 하루 근무 시간 기준) */
export function minutesLabel(min: number, dayMinutes?: number): string {
  if (dayMinutes && min >= dayMinutes) {
    const d = Math.floor(min / dayMinutes);
    const rest = min - d * dayMinutes;
    return `${d}영업일${rest ? ` ${minutesLabel(rest)}` : ''}`;
  }
  if (min < 60) return `${min}분`;
  const h = Math.floor(min / 60);
  const m = min % 60;
  if (!dayMinutes && h >= 48 && m === 0 && h % 24 === 0) return `${h / 24}일`;
  return m ? `${h}시간 ${m}분` : `${h}시간`;
}
export const WEEKDAY = ['', '월', '화', '수', '목', '금', '토', '일'];

// ---------- Phase 3: CRM Lite ----------
export const REL_KIND: Record<RelKind, { label: string; icon: string }> = {
  ARTIST: { label: '아티스트', icon: 'music' }, LABEL: { label: '레이블', icon: 'users' }, DSP: { label: 'DSP', icon: 'globe' },
  PARTNER: { label: '파트너', icon: 'user' }, ORG: { label: '작업 공간', icon: 'user' },
};
export const REL_KINDS = Object.keys(REL_KIND) as RelKind[];
export const REL_STATUS: Record<RelStatus, [string, Tone]> = {
  PROSPECT: ['잠재', 'violet'], ACTIVE: ['활성', 'green'], AT_RISK: ['주의', 'amber'], INACTIVE: ['비활성', 'gray'],
};
export const REL_STATUSES = Object.keys(REL_STATUS) as RelStatus[];
export const CONTRACT_STATUS: Record<ContractStatus, [string, Tone]> = {
  NONE: ['계약 없음', 'gray'], NEGOTIATING: ['진행 중', 'amber'], SIGNED: ['체결', 'green'], EXPIRED: ['만료', 'gray'], TERMINATED: ['해지', 'red'],
};
export const CONTRACT_STATUSES = Object.keys(CONTRACT_STATUS) as ContractStatus[];
export const ACTIVITY_KIND: Record<ActivityKind, { label: string; icon: string }> = {
  CALL: { label: '통화', icon: 'inquiry' }, EMAIL: { label: '메일', icon: 'mail' }, MEETING: { label: '미팅', icon: 'users' }, NOTE: { label: '메모', icon: 'pencil' },
};
export const ACTIVITY_KINDS = Object.keys(ACTIVITY_KIND) as ActivityKind[];
/** 관계 종류 → 원천 Entity 종류 (파트너는 원천 없음) */
export const REL_ENTITY: Partial<Record<RelKind, EntityKind>> = { ARTIST: 'artist', LABEL: 'label', DSP: 'dsp', ORG: 'org' };
export const pct = (v: number) => `${Math.round(v * 1000) / 10}%`;

// ---------- 칸반 ----------
/** 우선순위는 색만이 아니라 P0–P3 글자로도 보여 준다 */
export const PRIORITY_CODE: Record<Priority, string> = { URGENT: 'P0', HIGH: 'P1', NORMAL: 'P2', LOW: 'P3' };
export const PRIORITY_NAME: Record<Priority, string> = { URGENT: 'Critical', HIGH: 'High', NORMAL: 'Normal', LOW: 'Low' };
export const CARD_SOURCE: Record<string, string> = {
  manual: '직접 등록', system: '시스템', qc: 'QC', distribution: '배급', support: '고객 지원', rights: '권리', finance: '정산', sla: 'SLA',
};
export const BOARD_KIND: Record<string, string> = { PERSONAL: '개인', TEAM: '팀', OPERATIONS: '운영' };
export const AUTO_TRIGGER: Record<string, string> = {
  QC_FAILURE: 'QC 실패', METADATA_CONFLICT: '메타데이터 충돌', MISSING_RIGHTS_DOCUMENT: '권리 서류 누락', DSP_REJECTION: 'DSP 반려',
  DISTRIBUTION_FAILURE: '배급(전송) 실패', SETTLEMENT_MISMATCH: '정산 불일치', AUTOMATION_FAILURE: '자동화 실패',
  SUPPORT_INQUIRY: '새 고객 문의', APPROVAL_REQUIRED: '2차 승인 필요', RELEASE_REVIEW_OVERDUE: '발매 심사 기한 초과',
};

// ---------- 문서 ----------
export const DOC_STATUS: Record<string, [string, Tone]> = {
  DRAFT: ['초안', 'gray'], IN_REVIEW: ['검토 중', 'violet'], APPROVED: ['승인됨', 'green'],
};
export const DOC_LEVEL: Record<string, string> = { OWNER: '소유자', MANAGER: '관리', EDITOR: '편집', VIEWER: '보기', NONE: '없음' };
