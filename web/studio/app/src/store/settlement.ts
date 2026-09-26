// 정산·지급 기록 스토어 — Settlement 화면과 Dashboard 요약이 공유
import { createStore } from '../lib/store';

export interface Statement { id: string; period: string; platform: string; amount: number; note: string; created: string }
export interface Payout { id: string; amount: number; note: string; created: string; status: 'recorded' | 'sent' }

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

export const statementsStore = createStore<Statement[]>(INITIAL_STATEMENTS, { persist: 'statements', revive: validRows });
export const payoutsStore = createStore<Payout[]>(INITIAL_PAYOUTS, { persist: 'payouts', revive: validRows });

export function balance(statements: Statement[], payouts: Payout[]) {
  const total = statements.reduce((n, s) => n + (Number(s.amount) || 0), 0);
  const used = payouts.reduce((n, p) => n + (Number(p.amount) || 0), 0);
  return { total, used, left: Math.max(0, total - used) };
}
