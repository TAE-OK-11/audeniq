// 발매 카운트다운 — 발매일(과 동시 발매 시각)은 한국 시간 기준. 시각이 없으면 그날 0시.

/** 발매 순간 (밀리초) */
export function releaseMoment(date: string, time?: string | null): number {
  const t = time && /^\d{2}:\d{2}$/.test(time) ? time : '00:00';
  return Date.parse(`${date}T${t}:00+09:00`);
}

/** 남은 시간 — 지났으면 null. 'D-3 · 12:04:31', 하루 안이면 '12:04:31' */
export function countdownLabel(target: number, now = Date.now()): string | null {
  const left = Math.floor((target - now) / 1000);
  if (left <= 0) return null;
  const d = Math.floor(left / 86400);
  const two = (n: number) => String(n).padStart(2, '0');
  const clock = `${two(Math.floor((left % 86400) / 3600))}:${two(Math.floor((left % 3600) / 60))}:${two(left % 60)}`;
  return d > 0 ? `D-${d} · ${clock}` : clock;
}

/** 남은 시간을 칸별로 — 지났으면 null. 시·분·초는 두 자리 문자열 */
export function countdownParts(target: number, now = Date.now()): { d: number; h: string; m: string; s: string } | null {
  const left = Math.floor((target - now) / 1000);
  if (left <= 0) return null;
  const two = (n: number) => String(n).padStart(2, '0');
  return { d: Math.floor(left / 86400), h: two(Math.floor((left % 86400) / 3600)), m: two(Math.floor((left % 3600) / 60)), s: two(left % 60) };
}
