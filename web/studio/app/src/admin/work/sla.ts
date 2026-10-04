// SLA 계산 — 업무 유형별 제한 시간, 영업시간(평일 09–18시 한국 시간), Due Soon / Overdue, escalation, 긴급도 자동 상승.
// 백엔드(crates/core/src/work.rs)의 같은 규칙을 화면에서도 즉시 보여 주려고 순수 함수로 둔다.
import type { Priority, SlaPolicy, SlaState, TaskStatus, TaskType } from './types';

const MIN = 60_000;
const KST = 9 * 60 * MIN;
const OPEN_H = 9;
const CLOSE_H = 18;

const kst = (ms: number) => new Date(ms + KST);
const isWeekend = (d: Date) => d.getUTCDay() === 0 || d.getUTCDay() === 6;
/** 한국 시간 그날 h시의 실제 시각(ms) */
const atHour = (d: Date, h: number) => Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate(), h) - KST;

/** 다음 날 09시 */
const nextOpen = (d: Date) => atHour(kst(atHour(d, 0) + 24 * 60 * MIN), OPEN_H);

/** start부터 영업시간만 minutes만큼 지난 시각 */
export function addBusinessMinutes(startMs: number, minutes: number): number {
  let t = startMs;
  let left = minutes * MIN;
  for (let guard = 0; guard < 4000; guard++) {
    const d = kst(t);
    const open = atHour(d, OPEN_H);
    const close = atHour(d, CLOSE_H);
    if (isWeekend(d) || t >= close) { t = nextOpen(d); continue; }
    if (t < open) t = open;
    const room = close - t;
    if (left <= room) return t + left;
    left -= room;
    t = close;
  }
  return t + left;
}

/** 두 시각 사이의 영업시간(분) */
export function businessMinutesBetween(fromMs: number, toMs: number): number {
  if (toMs <= fromMs) return 0;
  let t = fromMs;
  let total = 0;
  for (let guard = 0; guard < 4000 && t < toMs; guard++) {
    const d = kst(t);
    const open = atHour(d, OPEN_H);
    const close = atHour(d, CLOSE_H);
    if (isWeekend(d) || t >= close) { t = nextOpen(d); continue; }
    if (t < open) t = open;
    const end = Math.min(close, toMs);
    if (end > t) total += end - t;
    t = close;
  }
  return Math.round(total / MIN);
}

export function slaDueAt(createdIso: string, policy: SlaPolicy | undefined): string | null {
  if (!policy || !policy.enabled) return null;
  const start = Date.parse(createdIso);
  const due = policy.business_hours ? addBusinessMinutes(start, policy.target_minutes) : start + policy.target_minutes * MIN;
  return new Date(due).toISOString();
}

/** 마감일과 SLA 기한 중 빠른 쪽 */
export function effectiveDue(t: { due_at: string | null; sla_due_at: string | null }): string | null {
  const a = t.due_at ? Date.parse(t.due_at) : Infinity;
  const b = t.sla_due_at ? Date.parse(t.sla_due_at) : Infinity;
  const m = Math.min(a, b);
  return Number.isFinite(m) ? new Date(m).toISOString() : null;
}

const CLOSED: TaskStatus[] = ['DONE', 'CANCELLED'];

export function slaState(
  t: { status: TaskStatus; created_at: string; completed_at: string | null; due_at: string | null; sla_due_at: string | null },
  warnRatio = 0.25,
  now = Date.now(),
): SlaState {
  const due = effectiveDue(t);
  if (!due) return 'NONE';
  const dueMs = Date.parse(due);
  if (CLOSED.includes(t.status)) {
    if (t.status === 'CANCELLED') return 'NONE';
    return t.completed_at && Date.parse(t.completed_at) <= dueMs ? 'MET' : 'MISSED';
  }
  if (now > dueMs) return 'OVERDUE';
  const total = dueMs - Date.parse(t.created_at);
  return total > 0 && dueMs - now <= total * warnRatio ? 'DUE_SOON' : 'OK';
}

const ORDER: Priority[] = ['LOW', 'NORMAL', 'HIGH', 'URGENT'];
export const raisePriority = (p: Priority): Priority => ORDER[Math.min(ORDER.length - 1, ORDER.indexOf(p) + 1)];
export const priorityRank = (p: Priority) => ORDER.indexOf(p);

/** SLA 상태 → escalation 단계와 자동 우선순위.
 *  Due Soon이면 1단계(담당자 알림) + 한 단계 상승, Overdue면 2단계(책임자) + 긴급. */
export function escalate(state: SlaState, priority: Priority, policy: SlaPolicy | undefined): { level: number; priority: Priority } {
  const raise = !!policy?.auto_raise_priority;
  if (state === 'OVERDUE') return { level: 2, priority: raise ? 'URGENT' : priority };
  if (state === 'DUE_SOON') return { level: 1, priority: raise ? raisePriority(priority) : priority };
  return { level: 0, priority };
}

/** 사람이 읽는 남은 시간 — '2시간 남음', '35분 지남' */
export function remaining(iso: string | null, now = Date.now()): string {
  if (!iso) return '';
  const ms = Date.parse(iso) - now;
  const past = ms < 0;
  const m = Math.round(Math.abs(ms) / MIN);
  const text = m < 60 ? `${m}분` : m < 48 * 60 ? `${Math.round(m / 60)}시간` : `${Math.round(m / 1440)}일`;
  return past ? `${text} 지남` : `${text} 남음`;
}

/** 분 → '2시간', '1영업일' 같은 제한 시간 표기 */
export function durationLabel(minutes: number, business: boolean): string {
  if (business && minutes % 540 === 0) return `${minutes / 540}영업일`;
  if (minutes < 60) return `${minutes}분`;
  if (minutes % 1440 === 0 && !business) return `${minutes / 1440}일`;
  const h = minutes / 60;
  return `${Number.isInteger(h) ? h : h.toFixed(1)}${business ? '영업시간' : '시간'}`;
}

export const DEFAULT_SLA: SlaPolicy[] = ([
  ['URGENT_DELIVERY', 120, false, 30, 'OPERATOR'],
  ['RIGHTS', 540, true, 120, 'REVIEWER'],
  ['RELEASE', 1080, true, 240, 'REVIEWER'],
  ['DSP_INQUIRY', 540, true, 120, 'OPERATOR'],
  ['SUPPORT', 540, true, 60, 'SUPPORT'],
  ['CONTRACT', 1620, true, 540, 'ADMIN'],
  ['SETTLEMENT', 1620, true, 540, 'ADMIN'],
  ['LEGAL', 2700, true, 540, 'ADMIN'],
  ['GENERAL', 2700, true, 1080, 'ADMIN'],
] as [TaskType, number, boolean, number, SlaPolicy['escalate_to']][]).map(([type, target_minutes, business_hours, idle_alert_minutes, escalate_to]) => ({
  type, target_minutes, business_hours, warn_ratio: 0.25, idle_alert_minutes, escalate_to, auto_raise_priority: true, enabled: true,
}));
