// 미리듣기·숏폼 클립 시작 지점 — 화면은 'm:ss', 서버는 초
/** 'm:ss' 또는 초 → 서버에 보낼 초 (비었거나 형식이 틀리면 '') */
export function previewSecs(v: string | undefined): string {
  const s = (v ?? '').trim();
  if (!s) return '';
  const m = /^(\d{1,2}):([0-5]\d)$/.exec(s);
  if (m) { const n = Number(m[1]) * 60 + Number(m[2]); return n <= 3600 ? String(n) : ''; }
  return /^\d{1,4}$/.test(s) && Number(s) <= 3600 ? String(Number(s)) : '';
}
/** 입력하는 대로 'm:ss'로 — 휴대폰 숫자 자판에는 ':'가 없다. 숫자만 치면 뒤 두 자리를 초로 본다 (0310 → 3:10, 105 → 1:05) */
export function maskPreviewInput(raw: string): string {
  const d = raw.replace(/\D/g, '').slice(0, 4);
  if (d.length <= 2) return d;
  return `${Number(d.slice(0, -2))}:${d.slice(-2)}`;
}
/** 칸을 떠날 때 — 초만 적었으면(45, 90) 'm:ss'로 맞춘다 */
export function settlePreviewInput(v: string): string {
  const secs = previewSecs(v);
  return secs ? previewLabel(secs) : v;
}
/** 저장된 초 → 화면 'm:ss' */
export function previewLabel(v: string | number | undefined): string {
  const n = typeof v === 'number' ? v : /^\d+$/.test(String(v ?? '')) ? Number(v) : NaN;
  return Number.isFinite(n) ? `${Math.floor(n / 60)}:${String(n % 60).padStart(2, '0')}` : '';
}
