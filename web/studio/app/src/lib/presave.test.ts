import { describe, expect, it } from 'vitest';
import { presaveResult, releaseIcs, withoutPresaveQuery } from './presave';

describe('프리세이브', () => {
  it('플랫폼이 돌려보낸 결과를 문장으로', () => {
    expect(presaveResult('?presave=spotify')).toEqual({ ok: true, text: 'Spotify에 미리 저장해 뒀어요. 발매되는 순간 보관함에 들어가요.' });
    const err = presaveResult('?presave_error=PRESAVE_DENIED');
    expect(err?.ok).toBe(false);
    expect(err?.text).toContain('허락');
    expect(presaveResult('?x=1')).toBeNull();
  });

  it('결과 쿼리만 지운다', () => {
    expect(withoutPresaveQuery('?presave=deezer')).toBe('');
    expect(withoutPresaveQuery('?a=1&presave_error=PRESAVE_FAILED')).toBe('?a=1');
  });

  it('발매 순간(한국 시간)을 UTC 일정으로, 글자는 이스케이프', () => {
    const ics = releaseIcs({ title: '여름, 끝; 둘', artist: '한결', date: '2026-11-15', time: '18:00', url: 'https://studio.audeniq.com/l/abcd2345' }, Date.UTC(2026, 9, 24));
    expect(ics).toContain('DTSTART:20261115T090000Z');
    expect(ics).toContain('DTEND:20261115T093000Z');
    expect(ics).toContain('SUMMARY:한결 - 여름\\, 끝\; 둘 발매');
    expect(ics).toContain('URL:https://studio.audeniq.com/l/abcd2345');
    expect(ics.split('\r\n')[0]).toBe('BEGIN:VCALENDAR');
    expect(releaseIcs({ title: 'a', artist: null, date: '2026-11-15', url: 'u' })).toContain('DTSTART:20261114T150000Z');
  });
});
