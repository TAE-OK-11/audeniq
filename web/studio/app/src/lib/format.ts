// 날짜/금액 포맷 헬퍼
import { parseStamp } from './date';

export const dateOnly = (d: unknown): string => String(d || '').slice(0, 10);

export const niceDate = (d: unknown): string => dateOnly(d) || '날짜 없음';

// 포매터는 모듈 레벨에 한 번만 생성 (호출마다 생성하면 GC/연산 낭비)
const numFmt = new Intl.NumberFormat('ko-KR');
const stampFmt = new Intl.DateTimeFormat('ko-KR', { dateStyle: 'long', timeStyle: 'short' });
const dayFmt = new Intl.DateTimeFormat('ko-KR', { dateStyle: 'long' });

/** 'YYYY-MM-DD HH:mm' 등을 '2026년 9월 26일 오후 2:32'로. 시간이 없으면 날짜만 */
export function localStamp(v: unknown): string {
  const dt = parseStamp(v);
  if (!dt) return '기록 없음';
  const hasTime = /\d{2}:\d{2}/.test(String(v));
  return hasTime ? stampFmt.format(dt) : dayFmt.format(dt);
}

/** 금액은 ‘12,000원’·‘15달러’처럼 읽히는 말로 (₩·$ 기호 대신) */
export function money(n: number, currency = 'KRW'): string {
  const v = Number.isFinite(n) ? n : 0;
  if (currency === 'USD') return `${new Intl.NumberFormat('ko-KR', { maximumFractionDigits: 2 }).format(v)}달러`;
  return `${numFmt.format(Math.round(v))}원`;
}

/** 수입이 아직 없을 때 ‘0원’ 대신 보여 줄 말 */
export const NO_INCOME = '아직 수입이 발생하지 않았어요';

export function num(n: number): string {
  return numFmt.format(Number.isFinite(n) ? n : 0);
}

export function fileSize(bytes: number): string {
  if (!bytes) return '';
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))}KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)}MB`;
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
  scheduled: '배급 승인',
  live: '발매 완료',
  closed: '진행 종료',
  rejected: '발매 거절',
  cancelled: '신청 취소',
};
