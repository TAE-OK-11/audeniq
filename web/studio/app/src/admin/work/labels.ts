// 업무 모듈 표기 — 상태·우선순위·유형을 한국어 라벨과 칩 색(Tone)으로. 관리자 화면의 labels.ts와 같은 방식.
import type { Tone } from '../labels';
import type { EntityKind, Priority, RecurFreq, TaskStatus, WorkType } from './types';

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
};
/** 연결할 때 고르는 순서 */
export const LINK_KINDS: EntityKind[] = ['release', 'artist', 'label', 'org', 'document', 'dsp', 'inquiry', 'settlement_report', 'payout', 'task'];

/** Entity가 열리는 관리자 주소 (화면이 없는 종류는 null) */
export function entityLink(kind: EntityKind, id: string): string | null {
  switch (kind) {
    case 'release': return `/admin/reviews/${id}`;
    case 'inquiry': return `/admin/inquiries/${id}`;
    case 'dsp': return '/admin/dsps';
    case 'document': return '/admin/documents';
    case 'payout': return '/admin/payouts';
    case 'task': return `/admin/tasks/${id}`;
    default: return null;
  }
}

export const DOC_KIND_LABEL: Record<string, string> = { AGREEMENT: '배급 계약서', RIGHTS_PROOF: '권리 증빙' };

export const EVENT_LABEL: Record<string, string> = {
  CREATED: '업무를 만들었어요', ASSIGNED: '담당자를 정했어요', REVIEWER: '검토자를 정했어요', STATUS: '상태를 바꿨어요',
  PRIORITY: '우선순위를 바꿨어요', TYPE: '업무 유형을 바꿨어요', DUE: '마감일을 바꿨어요', RECURRENCE: '반복 설정을 바꿨어요',
  REVIEW_REQUESTED: '검토를 요청했어요', APPROVED: '검토를 승인했어요', CHANGES_REQUESTED: '보완을 요청했어요',
  RECURRED: '반복 업무로 다음 건이 만들어졌어요', EDITED: '내용을 고쳤어요', NOTE: '메모를 남겼어요',
  LINKED: '연결했어요', UNLINKED: '연결을 해제했어요', SUBTASK_ADDED: '하위 업무를 추가했어요', ARCHIVED: '보관했어요',
};

/** 사람이 읽는 남은 시간 — '2시간 남음', '35분 지남' */
export function remaining(iso: string | null, now = Date.now()): string {
  if (!iso) return '';
  const ms = Date.parse(iso) - now;
  const m = Math.round(Math.abs(ms) / 60_000);
  const text = m < 60 ? `${m}분` : m < 48 * 60 ? `${Math.round(m / 60)}시간` : `${Math.round(m / 1440)}일`;
  return ms < 0 ? `${text} 지남` : `${text} 남음`;
}
