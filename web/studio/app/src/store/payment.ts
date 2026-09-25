// 수령(지급) 정보 공유 스토어 — 라이브 db.payment 대응
// Profile(등록/변경)과 Settlement(수익 받기)가 같은 상태를 공유한다.
import { useSyncExternalStore } from 'react';

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

// 라이브 초기값: payment:{recipient:'',type:'personal',bank:'',accountNumber:'',last4:''}
let payment: PaymentInfo | null = null;
const listeners = new Set<() => void>();

function emit() {
  listeners.forEach(l => l());
}

function subscribe(l: () => void): () => void {
  listeners.add(l);
  return () => { listeners.delete(l); };
}

export function getPayment(): PaymentInfo | null {
  return payment;
}

/** 라이브 등록 판정: recipient && bank && bank!=='미설정' && accountNumber && last4 && last4!=='0000' */
export function isPaymentRegistered(p: PaymentInfo | null): boolean {
  return !!p && !!p.recipient && !!p.bank && p.bank !== '미설정' && !!p.accountNumber && !!p.last4 && p.last4 !== '0000';
}

export function setPayment(p: PaymentInfo): void {
  payment = p;
  emit();
}

export function usePayment(): PaymentInfo | null {
  return useSyncExternalStore(subscribe, getPayment);
}
