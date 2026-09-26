// 라이브 HTML의 날짜/금액 포맷 헬퍼 (studio_live.html 기준)
export const dateOnly = (d: unknown): string => String(d || '').slice(0, 10);

export const niceDate = (d: unknown): string => dateOnly(d) || '날짜 없음';

// 포매터는 모듈 레벨에 한 번만 생성 (호출마다 생성하면 GC/연산 낭비)
const krwFmt = new Intl.NumberFormat('ko-KR', { style: 'currency', currency: 'KRW', maximumFractionDigits: 0 });
const numFmt = new Intl.NumberFormat('ko-KR');
const stampFmt = new Intl.DateTimeFormat('ko-KR', { dateStyle: 'long', timeStyle: 'medium' });

export function localStamp(v: unknown): string {
  if (!v) return '기록 없음';
  const dt = new Date(String(v));
  if (Number.isNaN(dt.getTime())) return '기록 없음';
  return stampFmt.format(dt);
}

export function money(n: number): string {
  return krwFmt.format(n || 0);
}

export function num(n: number): string {
  return numFmt.format(n || 0);
}

/** 계약서 카드 제목 뒤의 '· 샘플' 접미사 제거 (aqDocumentCards 기준) */
export function stripSampleSuffix(title: string): string {
  return title.replace(/\s*[·•]\s*샘플\s*$/u, '');
}

/** 발매 상태 라벨 — 라이브 statusLabel (칩 클래스는 상태값 그대로 사용) */
export const STATUS_LABEL: Record<string, string> = {
  draft: '작성 중',
  ready: '접수 대기',
  needs: '보완 필요',
  review: '검토 중',
  scheduled: '발매 예정',
  live: '발매 완료',
};
