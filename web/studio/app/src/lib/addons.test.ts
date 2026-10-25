import { describe, expect, it } from 'vitest';
import { lrcLines, orderStage } from './addons';

describe('add-ons', () => {
  it('checks LRC the way the server does', () => {
    expect(lrcLines('[ti:여름의 끝]\n\n[00:12.30]첫 줄\n[00:15.00][01:02.50]후렴\n[03:01]끝')).toBe(3);
    for (const bad of ['', '그냥 가사', '[00:12.30]첫 줄\n시간 없는 줄', '[00:75.00]초가 60 넘음', '[ti:제목만]']) expect(lrcLines(bad)).toBeNull();
  });

  it('shows where an order stands, and where a closed one stopped', () => {
    const ev = (...s: string[]) => s.map(status => ({ status, note: '', by_staff: false, at: '' }));
    expect(orderStage({ status: 'AWAITING_PAYMENT', events: ev('AWAITING_PAYMENT') })).toMatchObject({ current: 0, complete: false, stopped: false });
    expect(orderStage({ status: 'COMPLETED', events: ev('AWAITING_PAYMENT', 'SUBMITTED', 'COMPLETED') })).toMatchObject({ current: 4, complete: true });
    const cancelled = orderStage({ status: 'CANCELLED', events: ev('AWAITING_PAYMENT', 'SUBMITTED', 'CANCELLED') });
    expect(cancelled).toMatchObject({ current: 2, stopped: true });
    expect(cancelled.steps[2]).toBe('취소');
  });
});
