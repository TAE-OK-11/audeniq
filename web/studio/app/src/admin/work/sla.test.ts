import { describe, expect, it } from 'vitest';
import { addBusinessMinutes, businessMinutesBetween, durationLabel, escalate, raisePriority, remaining, slaDueAt, slaState, DEFAULT_SLA } from './sla';

// 2026-10-05은 월요일
const kst = (s: string) => Date.parse(`${s}+09:00`);

describe('영업시간 SLA', () => {
  it('영업시간 안에서는 그대로 더한다', () => {
    expect(addBusinessMinutes(kst('2026-10-05T10:00:00'), 120)).toBe(kst('2026-10-05T12:00:00'));
  });
  it('퇴근 시간을 넘기면 다음 영업일 09시부터 이어서 센다', () => {
    expect(addBusinessMinutes(kst('2026-10-05T17:00:00'), 120)).toBe(kst('2026-10-06T10:00:00'));
  });
  it('금요일 저녁 접수는 월요일로 넘어간다', () => {
    expect(addBusinessMinutes(kst('2026-10-09T19:30:00'), 540)).toBe(kst('2026-10-12T18:00:00'));
  });
  it('업무 시작 전 접수는 09시부터 센다', () => {
    expect(addBusinessMinutes(kst('2026-10-05T07:00:00'), 30)).toBe(kst('2026-10-05T09:30:00'));
  });
  it('두 시각 사이 영업시간을 센다', () => {
    expect(businessMinutesBetween(kst('2026-10-09T17:00:00'), kst('2026-10-12T10:00:00'))).toBe(120);
  });
  it('정책으로 기한을 계산한다 (긴급 송출 2시간, 권리 검토 1영업일)', () => {
    const urgent = DEFAULT_SLA.find(p => p.type === 'URGENT_DELIVERY')!;
    const rights = DEFAULT_SLA.find(p => p.type === 'RIGHTS')!;
    expect(Date.parse(slaDueAt('2026-10-05T22:00:00+09:00', urgent)!)).toBe(kst('2026-10-06T00:00:00'));
    expect(Date.parse(slaDueAt('2026-10-05T14:00:00+09:00', rights)!)).toBe(kst('2026-10-06T14:00:00'));
    expect(slaDueAt('2026-10-05T14:00:00+09:00', { ...rights, enabled: false })).toBeNull();
  });
});

describe('SLA 상태와 escalation', () => {
  const base = { status: 'TODO' as const, created_at: '2026-10-05T00:00:00Z', completed_at: null, due_at: null, sla_due_at: '2026-10-05T04:00:00Z' };
  it('남은 시간이 25% 이하면 Due Soon, 지나면 Overdue', () => {
    expect(slaState(base, 0.25, Date.parse('2026-10-05T01:00:00Z'))).toBe('OK');
    expect(slaState(base, 0.25, Date.parse('2026-10-05T03:30:00Z'))).toBe('DUE_SOON');
    expect(slaState(base, 0.25, Date.parse('2026-10-05T05:00:00Z'))).toBe('OVERDUE');
  });
  it('마감일이 SLA보다 빠르면 마감일 기준', () => {
    expect(slaState({ ...base, due_at: '2026-10-05T02:00:00Z' }, 0.25, Date.parse('2026-10-05T02:30:00Z'))).toBe('OVERDUE');
  });
  it('완료된 업무는 지켰는지만 본다', () => {
    expect(slaState({ ...base, status: 'DONE', completed_at: '2026-10-05T03:00:00Z' })).toBe('MET');
    expect(slaState({ ...base, status: 'DONE', completed_at: '2026-10-05T06:00:00Z' })).toBe('MISSED');
  });
  it('긴급도 자동 상승', () => {
    const p = DEFAULT_SLA[0];
    expect(escalate('DUE_SOON', 'NORMAL', p)).toEqual({ level: 1, priority: 'HIGH' });
    expect(escalate('OVERDUE', 'LOW', p)).toEqual({ level: 2, priority: 'URGENT' });
    expect(escalate('OVERDUE', 'LOW', { ...p, auto_raise_priority: false })).toEqual({ level: 2, priority: 'LOW' });
    expect(raisePriority('URGENT')).toBe('URGENT');
  });
  it('표기', () => {
    expect(durationLabel(120, false)).toBe('2시간');
    expect(durationLabel(540, true)).toBe('1영업일');
    expect(remaining('2026-10-05T02:00:00Z', Date.parse('2026-10-05T01:30:00Z'))).toBe('30분 남음');
    expect(remaining('2026-10-05T01:00:00Z', Date.parse('2026-10-05T04:00:00Z'))).toBe('3시간 지남');
  });
});
