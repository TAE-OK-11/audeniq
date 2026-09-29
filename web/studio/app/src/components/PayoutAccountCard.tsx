// 등록된 수령 계좌를 한눈에 보이는 카드로 — 은행·계좌 끝자리를 크게, 예금주·유형·등록일은 작게.
// 카드 전체를 누르면 계좌 변경(등록) 창이 열린다.
import { BankLogo } from './BankLogo';
import { Glyph } from './Glyph';
import { TYPE_LABEL, type PaymentInfo } from '../store/payment';
import { localStamp } from '../lib/format';

export function PayoutAccountCard({ payment, onEdit }: { payment: PaymentInfo | null; onEdit: () => void }) {
  if (!payment) {
    return (
      <button type="button" className="aq-acct aq-acct-empty" onClick={onEdit}>
        <span className="aq-acct-plus" aria-hidden="true"><Glyph name="plus" size={20} /></span>
        <span className="aq-acct-empty-copy">
          <strong>수익을 받을 계좌 등록하기</strong>
          <small>등록하면 정산·지급 화면에서 바로 수익을 받을 수 있어요.</small>
        </span>
        <Glyph name="chevron-right" size={16} />
      </button>
    );
  }
  return (
    <button type="button" className="aq-acct" onClick={onEdit} aria-label={`${payment.bank} 끝자리 ${payment.last4} 계좌 — 눌러서 변경`}>
      <span className="aq-acct-head">
        <BankLogo name={payment.bank} />
        <span className="aq-acct-bank">
          <strong>{payment.bank}</strong>
          <small>수익 정산 계좌</small>
        </span>
        <span className="aq-acct-status"><i aria-hidden="true" />등록 완료</span>
      </span>
      <span className="aq-acct-number" aria-hidden="true">
        <span className="aq-acct-dots">•••• ••••</span> {payment.last4}
      </span>
      <span className="aq-acct-meta">
        <span className="aq-acct-chip"><em>예금주</em>{payment.recipient}</span>
        <span className="aq-acct-chip">{TYPE_LABEL[payment.type]}</span>
      </span>
      <span className="aq-acct-foot">
        <small>{payment.registeredAt ? `${localStamp(payment.registeredAt)} 등록` : ''}</small>
        <span className="aq-acct-edit">계좌 변경 <Glyph name="chevron-right" size={14} /></span>
      </span>
    </button>
  );
}
