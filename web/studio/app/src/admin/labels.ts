// 관리자 화면 표기 — 서버 상태·검사 코드를 한국어 라벨과 칩 색(tone)으로.
import { correctionTarget, isKnownCorrection } from '../lib/corrections';
import type { Duty, StaffRole } from './api';

export type Tone = 'blue' | 'violet' | 'amber' | 'green' | 'red' | 'gray';

export const ROLE_LABEL: Record<StaffRole, string> = {
  ADMIN: '관리자', REVIEWER: '심사 담당', OPERATOR: '배급 운영', SUPPORT: '고객 지원',
};

export const DUTY_LABEL: Record<Duty, string> = {
  REVIEW: '발매 심사', DOCUMENTS: '서류 검토', INQUIRIES: '문의 답변', DELIVERY: '배급 승인',
};

export const RELEASE_STATUS: Record<string, [string, Tone]> = {
  SUBMITTED: ['접수됨', 'blue'],
  STAGE1_RUNNING: ['1차 검사 중', 'blue'],
  STAGE1_CORRECTION: ['1차 보완 요청', 'amber'],
  STAGE1_PASSED: ['1차 통과', 'blue'],
  STAGE2_RUNNING: ['2차 검사 중', 'blue'],
  STAGE2_REVIEW: ['심사 대기', 'violet'],
  STAGE2_CORRECTION: ['2차 보완 요청', 'amber'],
  STAGE2_PASSED: ['2차 통과', 'blue'],
  STAGE3_PREPARING: ['배포 준비 중', 'blue'],
  STAGE3_CORRECTION: ['배포 보완 요청', 'amber'],
  READY_FOR_DELIVERY: ['배급 준비 완료', 'green'],
  ON_HOLD_RIGHTS: ['권리 보류', 'red'],
  WITHDRAWN: ['거절·철회', 'gray'],
};

/** 심사 목록 필터 (백엔드 RELEASE_STATUSES 중 담당자가 자주 보는 순서) */
export const QUEUE_FILTERS = [
  'STAGE2_REVIEW', 'STAGE1_CORRECTION', 'STAGE2_CORRECTION', 'STAGE3_CORRECTION',
  'ON_HOLD_RIGHTS', 'READY_FOR_DELIVERY', 'SUBMITTED', 'STAGE3_PREPARING', 'WITHDRAWN',
];

export const CHECK_STATUS: Record<string, [string, Tone]> = {
  PASS: ['통과', 'green'],
  NOT_APPLICABLE: ['해당 없음', 'gray'],
  REVIEW_REQUIRED: ['검토 필요', 'violet'],
  CORRECTION_REQUIRED: ['보완 필요', 'amber'],
  BLOCKED: ['차단', 'red'],
  FAIL: ['실패', 'red'],
};

export const DOC_KIND: Record<string, string> = { AGREEMENT: '배급 계약서', RIGHTS_PROOF: '권리 증빙' };
export const DOC_STATUS: Record<string, [string, Tone]> = {
  AWAITING_DOCUMENTS: ['제출 대기', 'gray'],
  REVIEW: ['검토 대기', 'violet'],
  PREPARED: ['서명 준비', 'blue'],
  NEEDS: ['보완 요청', 'amber'],
  APPROVED: ['승인', 'green'],
  SIGNED: ['서명 완료', 'green'],
};

export const INQUIRY_STATUS: Record<string, [string, Tone]> = {
  OPEN: ['답변 대기', 'violet'], ANSWERED: ['답변 완료', 'green'], CLOSED: ['종료', 'gray'],
};

export const APPROVAL_STATUS: Record<string, [string, Tone]> = {
  PENDING: ['승인 대기', 'violet'], APPROVED: ['승인', 'green'], DECLINED: ['반려', 'gray'], HELD: ['보류', 'amber'],
};

export const READINESS: Record<string, [string, Tone]> = {
  READY: ['전송 가능', 'green'], AWAITING_PARTNER: ['파트너 연동 대기', 'blue'], CONTENT_BLOCKED: ['콘텐츠 차단', 'red'],
};

export const DECISION_LABEL: Record<string, string> = {
  APPROVE: '승인', REQUEST_CORRECTION: '보완 요청', REJECT: '거절',
};

export const PAYOUT_STATUS: Record<string, [string, Tone]> = {
  REQUESTED: ['요청됨', 'violet'], ORDERED: ['지급 지시', 'green'], REJECTED: ['반려', 'red'], CANCELLED: ['취소', 'gray'],
};

export const RELEASE_TYPE: Record<string, string> = { SINGLE: '싱글', EP: 'EP', ALBUM: '정규' };

/** 검사 코드 → 사람이 읽는 이름 (아티스트 화면의 보완 항목 매핑을 재사용) */
export function checkLabel(code: string): string {
  if (isKnownCorrection(code)) return correctionTarget(code).label;
  const extra: Record<string, string> = {
    S2_INTEGRITY_DUP: '중복 음원', AUDIO_SIMILAR_TO_EXISTING: '기존 음원과 유사', S2_PROTECTED_NAME: '보호 아티스트명',
    DSP_LOUDNESS_ADVISORY: '음량(라우드니스) 권고', DSP_CLIPPING_ADVISORY: '클리핑 권고',
  };
  return extra[code] ?? code;
}

export const pick = (map: Record<string, [string, Tone]>, key: string | null | undefined): [string, Tone] =>
  (key && map[key]) || [key || '—', 'gray'];

const stampFmt = new Intl.DateTimeFormat('ko-KR', { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
const dayFmt = new Intl.DateTimeFormat('ko-KR', { year: 'numeric', month: 'short', day: 'numeric' });

export function when(iso: string | null | undefined): string {
  if (!iso) return '—';
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? String(iso) : stampFmt.format(d);
}

export function day(iso: string | null | undefined): string {
  if (!iso) return '—';
  const d = new Date(iso.length === 10 ? `${iso}T00:00:00` : iso);
  return Number.isNaN(d.getTime()) ? String(iso) : dayFmt.format(d);
}

/** '3시간 전' 같은 상대 시각 — 대기열이 얼마나 밀렸는지 한눈에 */
export function ago(iso: string | null | undefined): string {
  if (!iso) return '';
  const ms = Date.now() - new Date(iso).getTime();
  if (!Number.isFinite(ms)) return '';
  const m = Math.round(ms / 60000);
  if (m < 1) return '방금';
  if (m < 60) return `${m}분 전`;
  const h = Math.round(m / 60);
  if (h < 48) return `${h}시간 전`;
  return `${Math.round(h / 24)}일 전`;
}

export const shortId = (v: string | null | undefined) => (v ? v.slice(0, 8) : '—');

/** 한 명이 통과시킬 수 있는 저위험 검사 (백엔드 review::LOW_RISK_SELF_APPROVABLE와 같게 유지) */
export const LOW_RISK = ['S2_RELEASE_DATE_FAR_FUTURE', 'S2_RELEASE_DATE_FAR_PAST', 'S2_META_CREDITS'];

/** 승인 시 두 번째 심사자가 필요한 항목 — staff::sensitive와 같은 규칙 */
export const needsSecond = (c: { check_code: string; status: string }) => c.status === 'BLOCKED' || !LOW_RISK.includes(c.check_code);

/** review::MAX_OVERRIDE_REASON_CHARS */
export const MAX_REASON = 2000;
