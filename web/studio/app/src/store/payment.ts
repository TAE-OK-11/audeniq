// 수령(지급) 정보 공유 스토어 — Profile/Settlement가 같은 상태를 공유
// 보안: 계좌번호 전체값은 브라우저 저장소에 남기지 않고 마스킹된 값만 영속화한다.
import { createStore } from '../lib/store';

export interface PaymentInfo {
  recipient: string;
  type: 'personal' | 'business' | 'corporate';
  bank: string;
  accountNumber: string;
  last4: string;
  registeredAt: string;
}

export const TYPE_LABEL: Record<PaymentInfo['type'], string> = {
  personal: '개인',
  business: '개인사업자',
  corporate: '법인',
};

const mask = (p: PaymentInfo): PaymentInfo => ({ ...p, accountNumber: p.last4 ? `••••${p.last4}` : '' });

const store = createStore<PaymentInfo | null>(null, {
  persist: 'payment',
  serialize: p => (p ? mask(p) : null),
});

export const getPayment = store.get;
export const usePayment = store.use;

/** 등록 판정 — 끝자리가 0000인 실제 계좌도 있으므로 끝자리 값으로 거르지 않는다 */
export function isPaymentRegistered(p: PaymentInfo | null): p is PaymentInfo {
  return !!p && !!p.recipient && !!p.bank && p.bank !== '미설정' && !!p.accountNumber && /^\d{4}$/.test(p.last4);
}

export function setPayment(p: PaymentInfo): void {
  store.set(p);
}
