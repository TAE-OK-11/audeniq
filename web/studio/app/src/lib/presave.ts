// 프리세이브 — 발매 전 스마트링크에서 팬이 플랫폼에 '보관함에 추가'를 미리 허락해 두면, 발매 순간 서버가 저장한다.
// 플랫폼이 돌려보낸 결과(?presave= / ?presave_error=)를 문장으로, 그리고 발매 순간을 캘린더 일정(.ics)으로.
import { messageForCode } from '../api/errors';
import { PLATFORM_NAMES } from './platformNames';
import { releaseMoment } from './countdown';

export interface PresaveResult { ok: boolean; text: string }

/** 주소의 결과 표시 — 없으면 null */
export function presaveResult(search: string): PresaveResult | null {
  const q = new URLSearchParams(search);
  const ok = q.get('presave');
  if (ok) {
    const name = PLATFORM_NAMES[ok] ?? ok;
    return { ok: true, text: `${name}에 미리 저장해 뒀어요. 발매되는 순간 보관함에 들어가요.` };
  }
  const code = q.get('presave_error');
  if (code) return { ok: false, text: messageForCode(/^[A-Z_]{1,60}$/.test(code) ? code : 'PRESAVE_FAILED') };
  return null;
}

/** 결과 표시용 쿼리를 뺀 주소 (새로고침해도 안내가 다시 뜨지 않게) */
export function withoutPresaveQuery(search: string): string {
  const q = new URLSearchParams(search);
  q.delete('presave');
  q.delete('presave_error');
  const s = q.toString();
  return s ? `?${s}` : '';
}

const icsText = (s: string) => s.replace(/\\/g, '\\\\').replace(/;/g, '\;').replace(/,/g, '\\,').replace(/\r?\n/g, '\\n');
const icsTime = (ms: number) => new Date(ms).toISOString().replace(/[-:]/g, '').replace(/\.\d{3}/, '');

export interface CalendarInput { title: string; artist: string | null; date: string; time?: string | null; url: string }

/** 발매 순간 30분 일정 + 시작할 때 알림 (UTC로 적어 어느 나라 캘린더에서도 같은 순간) */
export function releaseIcs(i: CalendarInput, now = Date.now()): string {
  const start = releaseMoment(i.date, i.time);
  const name = i.artist ? `${i.artist} - ${i.title}` : i.title;
  return [
    'BEGIN:VCALENDAR',
    'VERSION:2.0',
    'PRODID:-//AUDENIQ//Smart Link//KO',
    'CALSCALE:GREGORIAN',
    'BEGIN:VEVENT',
    `UID:${start}-${encodeURIComponent(i.url).slice(-40)}@audeniq`,
    `DTSTAMP:${icsTime(now)}`,
    `DTSTART:${icsTime(start)}`,
    `DTEND:${icsTime(start + 30 * 60 * 1000)}`,
    `SUMMARY:${icsText(`${name} 발매`)}`,
    `DESCRIPTION:${icsText(`지금 들으러 가기: ${i.url}`)}`,
    `URL:${i.url}`,
    'BEGIN:VALARM',
    'ACTION:DISPLAY',
    'TRIGGER:PT0M',
    `DESCRIPTION:${icsText(`${name} 발매`)}`,
    'END:VALARM',
    'END:VEVENT',
    'END:VCALENDAR',
    '',
  ].join('\r\n');
}
