// 업무 모듈 표기 — 상태·우선순위·유형을 한국어 라벨과 칩 색(Tone)으로. 관리자 화면의 labels.ts와 같은 방식.
import type { Tone } from '../labels';
import type {
  ActivityKind, ContactKind, ContractStage, EntityKind, ExceptionKind, ExceptionStatus, InboxKind,
  IntegrationHealth, IntegrationKind, Priority, RecurFreq, Severity, SlaState, TaskStatus, TaskType,
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

export const TASK_TYPE: Record<TaskType, string> = {
  URGENT_DELIVERY: '긴급 송출',
  RELEASE: '발매 업무',
  RIGHTS: '권리 검토',
  CONTRACT: '계약 검토',
  DSP_INQUIRY: 'DSP 문의',
  SETTLEMENT: '정산 확인',
  LEGAL: '법무 대응',
  SUPPORT: '고객 문의',
  GENERAL: '일반 업무',
};
export const TASK_TYPES = Object.keys(TASK_TYPE) as TaskType[];

export const SLA_STATE: Record<SlaState, [string, Tone]> = {
  NONE: ['기한 없음', 'gray'], OK: ['여유', 'green'], DUE_SOON: ['기한 임박', 'amber'], OVERDUE: ['기한 초과', 'red'],
  MET: ['기한 내 완료', 'green'], MISSED: ['기한 넘겨 완료', 'amber'],
};

export const RECUR: Record<RecurFreq, [string, string]> = {
  DAILY: ['매일', '일'], WEEKLY: ['매주', '주'], MONTHLY: ['매월', '개월'],
};
export const recurLabel = (r: { freq: RecurFreq; interval: number }) =>
  r.interval <= 1 ? RECUR[r.freq][0] : `${r.interval}${RECUR[r.freq][1]}마다`;

export const INBOX_KIND: Record<InboxKind, { label: string; icon: string; tone: Tone }> = {
  ASSIGNED: { label: '내 업무', icon: 'task', tone: 'blue' },
  REVIEW_REQUEST: { label: '승인 요청', icon: 'approval', tone: 'violet' },
  CORRECTION: { label: '보완 요청', icon: 'pencil', tone: 'amber' },
  MENTION: { label: '멘션', icon: 'at', tone: 'blue' },
  AUTOMATION_FAILED: { label: '실패한 자동화', icon: 'bolt', tone: 'red' },
  SLA_BREACH: { label: 'SLA 초과', icon: 'clock', tone: 'red' },
  INQUIRY: { label: '미처리 문의', icon: 'inquiry', tone: 'violet' },
  RIGHTS: { label: '권리 이슈', icon: 'shield', tone: 'amber' },
};
export const INBOX_KINDS = Object.keys(INBOX_KIND) as InboxKind[];

export const EXCEPTION_KIND: Record<ExceptionKind, { label: string; hint: string; icon: string }> = {
  QC: { label: 'QC 이상', hint: '음질·음량·파일 검사에서 걸린 건', icon: 'music' },
  RIGHTS_DOCS: { label: '권리 서류 부족', hint: '증빙·동의서가 빠졌거나 만료', icon: 'shield' },
  DSP_REJECTION: { label: 'DSP 반려', hint: '플랫폼이 거절한 전송', icon: 'globe' },
  METADATA_CONFLICT: { label: '메타데이터 충돌', hint: 'ISRC·아티스트명·크레딧 불일치', icon: 'doc' },
  SETTLEMENT_MISMATCH: { label: '정산 불일치', hint: '리포트와 지급액이 맞지 않음', icon: 'won' },
  DELIVERY_FAILURE: { label: '전송 실패', hint: 'SFTP·API 전송 오류', icon: 'truck' },
};
export const EXCEPTION_KINDS = Object.keys(EXCEPTION_KIND) as ExceptionKind[];

export const SEVERITY: Record<Severity, [string, Tone]> = {
  CRITICAL: ['치명', 'red'], HIGH: ['높음', 'amber'], MEDIUM: ['보통', 'blue'], LOW: ['낮음', 'gray'],
};
export const EXCEPTION_STATUS: Record<ExceptionStatus, [string, Tone]> = {
  OPEN: ['미배정', 'violet'], ASSIGNED: ['처리 중', 'blue'], RESOLVED: ['해결', 'green'], IGNORED: ['무시', 'gray'],
};

export const ENTITY_KIND: Record<EntityKind, { label: string; icon: string }> = {
  release: { label: '발매', icon: 'release' },
  org: { label: '작업 공간', icon: 'user' },
  contact: { label: '거래처', icon: 'users' },
  dsp: { label: 'DSP', icon: 'globe' },
  inquiry: { label: '문의', icon: 'inquiry' },
  document: { label: '서류', icon: 'doc' },
  exception: { label: '예외', icon: 'bolt' },
  payout: { label: '지급', icon: 'card' },
  task: { label: '업무', icon: 'task' },
};

/** 연결된 항목이 열리는 관리자 주소 */
export function entityLink(kind: EntityKind, id: string): string | null {
  switch (kind) {
    case 'release': return `/admin/reviews/${id}`;
    case 'inquiry': return `/admin/inquiries/${id}`;
    case 'contact': return `/admin/crm/${id}`;
    case 'dsp': return `/admin/dsp-ops?dsp=${encodeURIComponent(id)}`;
    case 'document': return '/admin/documents';
    case 'exception': return `/admin/exceptions?id=${encodeURIComponent(id)}`;
    case 'payout': return '/admin/payouts';
    case 'task': return `/admin/tasks/${id}`;
    default: return null;
  }
}

export const CONTACT_KIND: Record<ContactKind, string> = { ARTIST: '아티스트', LABEL: '레이블', DSP: 'DSP', PARTNER: '파트너' };
export const CONTACT_KINDS = Object.keys(CONTACT_KIND) as ContactKind[];
export const CONTRACT_STAGE: Record<ContractStage, [string, Tone]> = {
  NONE: ['계약 없음', 'gray'], NEGOTIATING: ['협의 중', 'violet'], SENT: ['계약서 발송', 'blue'],
  SIGNED: ['계약 완료', 'green'], RENEWAL: ['갱신 협의', 'amber'], ENDED: ['종료', 'gray'],
};
export const CONTRACT_STAGES = Object.keys(CONTRACT_STAGE) as ContractStage[];
export const ACTIVITY_KIND: Record<ActivityKind, string> = { CALL: '통화', EMAIL: '메일', MEETING: '미팅', NOTE: '메모' };

export const INTEGRATION: Record<IntegrationKind, string> = { DDEX_SFTP: 'DDEX · SFTP', DDEX_API: 'DDEX · API', API: '전용 API', MANUAL: '수동 전달' };
export const HEALTH: Record<IntegrationHealth, [string, Tone]> = {
  HEALTHY: ['정상', 'green'], DEGRADED: ['지연', 'amber'], DOWN: ['장애', 'red'], ONBOARDING: ['연동 중', 'blue'],
};

export const EVENT_LABEL: Record<string, string> = {
  CREATED: '업무를 만들었어요', ASSIGNED: '담당자를 정했어요', STATUS: '상태를 바꿨어요', PRIORITY: '우선순위를 바꿨어요',
  DUE: '마감일을 바꿨어요', REVIEW_REQUESTED: '검토를 요청했어요', APPROVED: '검토를 승인했어요',
  CHANGES_REQUESTED: '보완을 요청했어요', ESCALATED: 'SLA로 escalation 됐어요', PRIORITY_RAISED: '기한이 가까워 우선순위가 올라갔어요',
  RECURRED: '반복 업무로 다음 건이 만들어졌어요', EDITED: '내용을 고쳤어요', NOTE: '메모를 남겼어요',
};

/** 배급 메뉴 하위 탭 — 배급 현황 | DSP 운영 | 플랫폼별 조건 */
export const DSP_TABS = [{ to: '/admin/deliveries', label: '배급 현황' }, { to: '/admin/dsp-ops', label: 'DSP 운영' }, { to: '/admin/dsps', label: '플랫폼별 조건' }];
