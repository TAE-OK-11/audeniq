// 정산·지급 기록 스토어 — Settlement 화면과 Dashboard 요약이 공유
import { createStore } from '../lib/store';
import { MOCK } from '../lib/mode';

export interface Statement { id: string; period: string; platform: string; amount: number; note: string; created: string }
/** recorded = 체험 모드 기록, requested·processing·sent·failed·cancelled = 실서버 지급 요청 상태.
 *  fee·net: 신청 때 고정된 수수료·실수령액, cancellable: 송금 실행 전이라 취소할 수 있음 (실서버) */
export interface Payout {
  id: string; amount: number; note: string; created: string;
  status: 'recorded' | 'requested' | 'processing' | 'sent' | 'failed' | 'cancelled';
  fee?: number; net?: number; cancellable?: boolean;
}

/** 출금 수수료 (정산 약관 제9조): 신청금액 30,000원 이하 500원(부가가치세 포함), 초과 무료. 서버가 최종 계산한다. */
export function payoutFee(amount: number): number {
  return amount <= 30000 ? 500 : 0;
}

const INITIAL_STATEMENTS: Statement[] = [
  { id: 'st1', period: '2026-08', platform: 'Spotify', amount: 24406, note: '', created: '2026-09-10' },
  { id: 'st2', period: '2026-07', platform: 'Apple Music', amount: 18920, note: '', created: '2026-08-12' },
];

const INITIAL_PAYOUTS: Payout[] = [
  { id: 'p1', amount: 18920, note: '', created: '2026-08-15', status: 'recorded' },
];

// 금액이 숫자가 아닌 손상 데이터는 버린다 (잔액 계산이 NaN이 되는 것 방지)
const validRows = <T extends { id: string; amount: number }>(raw: unknown, fb: T[]): T[] =>
  (Array.isArray(raw)
    ? (raw as T[]).filter(r => r && typeof r.id === 'string' && Number.isFinite(Number(r.amount))).map(r => ({ ...r, amount: Number(r.amount) }))
    : fb);

export const statementsStore = createStore<Statement[]>(MOCK ? INITIAL_STATEMENTS : [], { persist: MOCK ? 'statements' : undefined, revive: validRows });
/** 실서버 정산 내역의 다음 쪽 위치 (null이면 전부 받음) */
export const statementsNextStore = createStore<string | null>(null);
export const payoutsStore = createStore<Payout[]>(MOCK ? INITIAL_PAYOUTS : [], { persist: MOCK ? 'payouts' : undefined, revive: validRows });

/** 실서버 정산 요약 (원장 기준 확정액·진행 중 요청·요청 가능액) */
/** held: 분쟁·중단으로 보류된 곡의 정산금 — 보류가 풀릴 때까지 요청할 수 없다 */
export interface FinanceState { payable: number; pending: number; available: number; held: number; minimum: number; loaded: boolean }
export const financeStore = createStore<FinanceState>({ payable: 0, pending: 0, available: 0, held: 0, minimum: 10000, loaded: false });

/** 화면용 잔액 — 체험 모드는 기록 합계, 실서버는 원장 요약 */
export function useBalance(): { total: number; used: number; left: number; minimum: number; held: number } {
  const statements = statementsStore.use();
  const payouts = payoutsStore.use();
  const fin = financeStore.use();
  if (MOCK) return { ...balance(statements, payouts), minimum: 1, held: 0 };
  return { total: fin.payable, used: fin.pending, left: fin.available, minimum: fin.minimum, held: fin.held };
}

export function balance(statements: Statement[], payouts: Payout[]) {
  const total = statements.reduce((n, s) => n + (Number(s.amount) || 0), 0);
  const used = payouts.reduce((n, p) => n + (Number(p.amount) || 0), 0);
  return { total, used, left: Math.max(0, total - used) };
}
