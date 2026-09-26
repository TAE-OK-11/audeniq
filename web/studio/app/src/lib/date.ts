// 날짜 유틸 — 로컬 타임존 기준 'YYYY-MM-DD HH:mm' 스탬프를 안전하게 만들고 파싱한다.
// Safari는 'YYYY-MM-DD HH:mm'(공백 구분) 문자열을 new Date()로 파싱하지 못하므로 직접 파싱한다.

const pad = (n: number) => String(n).padStart(2, '0');

/** 오늘 날짜 'YYYY-MM-DD' (로컬 기준) */
export function todayStr(d: Date = new Date()): string {
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** 현재 시각 'YYYY-MM-DD HH:mm' (로컬 기준) */
export function stampNow(d: Date = new Date()): string {
  return `${todayStr(d)} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** 'YYYY-MM' (로컬 기준) — offset만큼 월 이동 */
export function monthKey(offset = 0, d: Date = new Date()): string {
  const t = new Date(d.getFullYear(), d.getMonth() + offset, 1);
  return `${t.getFullYear()}-${pad(t.getMonth() + 1)}`;
}

const STAMP_RE = /^(\d{4})-(\d{2})-(\d{2})(?:[ T](\d{2}):(\d{2})(?::(\d{2}))?)?/;

/** 'YYYY-MM-DD', 'YYYY-MM-DD HH:mm', ISO 문자열을 Date로. 실패 시 null */
export function parseStamp(v: unknown): Date | null {
  if (v == null || v === '') return null;
  const s = String(v);
  // 타임존 표기가 있는 ISO 문자열은 브라우저 파서에 맡긴다
  if (/[zZ]|[+-]\d{2}:?\d{2}$/.test(s) && s.includes('T')) {
    const d = new Date(s);
    return Number.isNaN(d.getTime()) ? null : d;
  }
  const m = STAMP_RE.exec(s);
  if (!m) return null;
  const d = new Date(+m[1], +m[2] - 1, +m[3], +(m[4] ?? 0), +(m[5] ?? 0), +(m[6] ?? 0));
  return Number.isNaN(d.getTime()) ? null : d;
}

/** '2026년 9월 26일' */
export function formatKoreanDate(iso: string): string {
  const d = parseStamp(iso);
  if (!d) return iso || '';
  return `${d.getFullYear()}년 ${d.getMonth() + 1}월 ${d.getDate()}일`;
}

/** '방금 전', '3분 전', '2일 전' … 7일이 넘으면 날짜 */
export function relativeTime(v: unknown, now: Date = new Date()): string {
  const d = parseStamp(v);
  if (!d) return '';
  const diff = (now.getTime() - d.getTime()) / 1000;
  if (diff < 0) return todayStr(d);
  if (diff < 60) return '방금 전';
  if (diff < 3600) return `${Math.floor(diff / 60)}분 전`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}시간 전`;
  if (diff < 86400 * 7) return `${Math.floor(diff / 86400)}일 전`;
  return todayStr(d);
}

/** ISO 시각(UTC) → 한국 날짜 'YYYY-MM-DD' (공지 게시일 표기용) */
export function toKstDate(iso: string): string {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return iso.slice(0, 10);
  const d = new Date(t + 9 * 3_600_000);
  return `${d.getUTCFullYear()}-${String(d.getUTCMonth() + 1).padStart(2, '0')}-${String(d.getUTCDate()).padStart(2, '0')}`;
}
