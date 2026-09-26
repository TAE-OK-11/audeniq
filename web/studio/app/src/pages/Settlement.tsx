// 정산·지급 — hero + 탭 구조의 단순화된 레이아웃
import { useState } from 'react';
import { Modal } from '../components/Modal';
import { useToast } from '../components/Toast';
import { BankLogo } from '../components/BankLogo';
import { PaymentSetupModal } from '../components/PaymentSetupModal';
import { isPaymentRegistered, usePayment } from '../store/payment';
import { money, niceDate } from '../lib/format';

interface Statement { id: string; period: string; platform: string; amount: number; note: string; created: string }
interface Payout { id: string; amount: number; note: string; created: string; status: string }

const INITIAL_STATEMENTS: Statement[] = [
  { id: 'st1', period: '2026-08', platform: 'Spotify', amount: 24406, note: '', created: '2026-09-10' },
  { id: 'st2', period: '2026-07', platform: 'Apple Music', amount: 18920, note: '', created: '2026-08-12' },
];

const INITIAL_PAYOUTS: Payout[] = [
  { id: 'p1', amount: 18920, note: '', created: '2026-08-15', status: 'recorded' },
];

export function Settlement() {
  const toast = useToast();
  const payment = usePayment();
  const [statements, setStatements] = useState<Statement[]>(INITIAL_STATEMENTS);
  const [payouts, setPayouts] = useState<Payout[]>(INITIAL_PAYOUTS);
  const [tab, setTab] = useState<'statements' | 'payouts'>('statements');
  const [showPayout, setShowPayout] = useState(false);
  const [showPaySetup, setShowPaySetup] = useState(false);
  const [showPaymentSetup, setShowPaymentSetup] = useState(false);
  const [amount, setAmount] = useState('');
  const [payoutNote, setPayoutNote] = useState('');

  const paymentRegistered = isPaymentRegistered(payment);

  const total = statements.reduce((n, s) => n + Number(s.amount || 0), 0);
  const used = payouts.reduce((n, p) => n + Number(p.amount || 0), 0);
  const left = Math.max(0, total - used);

  const openPayoutModal = () => {
    if (!isPaymentRegistered(payment)) {
      toast('수익을 받을 정보를 먼저 등록해 주세요.');
      setShowPaySetup(true);
      return;
    }
    if (!left) { toast('지급을 요청할 수 있는 잔액이 없어요.'); return; }
    setAmount('');
    setPayoutNote('');
    setShowPayout(true);
  };

  const deleteStatement = (id: string) => {
    if (!window.confirm('선택한 내역을 삭제할까요?')) return;
    setStatements(ss => ss.filter(s => s.id !== id));
  };

  const deletePayout = (id: string) => {
    if (!window.confirm('선택한 내역을 삭제할까요?')) return;
    setPayouts(ps => ps.filter(p => p.id !== id));
  };

  const submitPayout = (e: React.FormEvent) => {
    e.preventDefault();
    const amt = Number(amount);
    if (!Number.isInteger(amt) || amt <= 0 || amt > left) {
      toast('요청 가능 금액 안에서 입력해 주세요.');
      return;
    }
    setPayouts(ps => [...ps, { id: 'p' + Date.now(), amount: amt, note: payoutNote.trim(), created: new Date().toISOString().slice(0, 10), status: 'recorded' }]);
    setShowPayout(false);
    toast('지급 요청 내역을 저장했어요.');
  };

  const orderedStatements = statements.slice().sort((a, b) => b.period.localeCompare(a.period));

  return (
    <div id="view-settlement" className="view">
      <div className="view-title">
        <div>
          <p className="eyebrow">ROYALTIES &amp; PAYOUTS</p>
          <h1>정산·지급</h1>
          <p>확정된 정산 금액과 지급 요청 내역을 확인해 보세요.</p>
        </div>
      </div>

      <section className="surface settle-hero" aria-labelledby="settleHeroHeading">
        <div className="settle-hero-top">
          <div>
            <span className="eyebrow">PAYOUT</span>
            <h2 id="settleHeroHeading">요청 전 잔액</h2>
            <strong className="settle-hero-amount">{money(left)}</strong>
            <p className="settle-hero-sub">
              기록한 정산액 {money(total)} · 지급 요청 합계 {money(used)}
            </p>
          </div>
          <button type="button" className="button" onClick={openPayoutModal}>수익 받기</button>
        </div>
        <div className="settle-hero-account">
          {paymentRegistered && payment ? (
            <>
              <BankLogo name={payment.bank} />
              <span className="min-0">{payment.bank} · •••• {payment.last4} · {payment.recipient}</span>
              <button type="button" className="link-btn" onClick={() => setShowPaymentSetup(true)}>변경</button>
            </>
          ) : (
            <>
              <span className="aq-payment-badge is-empty" aria-hidden="true">₩</span>
              <span className="min-0">수익을 받을 계좌를 등록해 주세요.</span>
              <button type="button" className="link-btn" onClick={() => setShowPaymentSetup(true)}>등록하기</button>
            </>
          )}
        </div>
      </section>

      <div className="tabs" role="tablist" aria-label="정산 내역 구분">
        <button
          type="button" role="tab" className="tab"
          aria-selected={tab === 'statements'}
          onClick={() => setTab('statements')}
        >
          정산 내역
        </button>
        <button
          type="button" role="tab" className="tab"
          aria-selected={tab === 'payouts'}
          onClick={() => setTab('payouts')}
        >
          지급 요청 기록
        </button>
      </div>

      {tab === 'statements' ? (
        <div id="statementList">
          {orderedStatements.length ? (
            <div className="aq-catalog-cards">
              {orderedStatements.map(s => (
                <div key={s.id} className="aq-statement-card">
                  <span className="aq-statement-icon" aria-hidden="true">₩</span>
                  <div className="min-0">
                    <span className="row-name">{s.period} · {s.platform}</span>
                    <span className="row-sub">{s.note || '수기 등록 정산 내역'} · {niceDate(s.created)}</span>
                  </div>
                  <div className="aq-statement-end">
                    <strong>{money(s.amount)}</strong>
                    <button type="button" className="link-btn" aria-label="정산 내역 삭제" onClick={() => deleteStatement(s.id)}>×</button>
                  </div>
                </div>
              ))}
            </div>
          ) : (
            <div className="empty-page">
              <h2>정산 내역이 없어요.</h2>
              <p>정산서를 받은 뒤 금액과 기간을 직접 기록할 수 있어요.</p>
            </div>
          )}
        </div>
      ) : (
        <div id="payoutList">
          {payouts.length ? (
            <div className="aq-catalog-cards">
              {[...payouts].reverse().map(p => (
                <div key={p.id} className="aq-statement-card">
                  <span className="aq-statement-icon" aria-hidden="true">↗</span>
                  <div className="min-0">
                    <span className="row-name">{money(p.amount)} · 지급 요청 기록</span>
                    <span className="row-sub">{niceDate(p.created)} · {p.note || '현재 작업 공간에만 기록됨'}</span>
                  </div>
                  <div className="aq-statement-end">
                    <span className="status-chip ready">전송 전</span>
                    <button type="button" className="link-btn" aria-label="요청 기록 삭제" onClick={() => deletePayout(p.id)}>×</button>
                  </div>
                </div>
              ))}
            </div>
          ) : (
            <div className="empty-page">
              <h2>지급 요청 기록이 없어요.</h2>
              <p>지급 요청 내용을 저장하면 이곳에 표시돼요.</p>
            </div>
          )}
        </div>
      )}

      {showPayout && (
        <Modal title="수익 지급 요청" onClose={() => setShowPayout(false)}>
          <div className="studio-payout-form">
            <p className="eyebrow">지급 요청 가능 금액</p>
            <strong className="studio-payout-number">{money(left)}</strong>
            <p className="small muted">원하는 금액을 입력하고 받으실 계좌를 확인해 주세요.</p>
            <form id="payoutForm" onSubmit={submitPayout}>
              <div className="field">
                <label htmlFor="pAmount">받을 금액 (원)</label>
                <div className="studio-amount-field">
                  <input
                    id="pAmount" inputMode="numeric" type="number" min={1} max={left} step={1}
                    required placeholder="금액을 입력해 주세요"
                    value={amount} onChange={e => setAmount(e.target.value)}
                  />
                  <button type="button" id="payoutAll" className="link-btn" onClick={() => setAmount(String(left))}>전액</button>
                </div>
                <p className="help">요청 가능 금액을 초과할 수 없어요.</p>
              </div>
              <div className="studio-receive-account">
                <BankLogo name={payment?.bank || ''} />
                <div>
                  <strong>{payment?.bank || '은행 미등록'}</strong>
                  <p className="small muted">
                    {payment?.recipient || '수령인 미등록'} · {payment?.last4 ? `•••• ${payment.last4}` : '계좌 미등록'}
                  </p>
                </div>
                <span className="studio-account-check" aria-label="선택된 수령 계좌">✓</span>
              </div>
              <div className="field">
                <label htmlFor="pNote">메모 (선택)</label>
                <input
                  id="pNote" maxLength={200} placeholder="필요한 내용을 남겨 주세요"
                  value={payoutNote} onChange={e => setPayoutNote(e.target.value)}
                />
              </div>
              <div className="doc-connection">
                신청 내용은 현재 작업 공간에 저장돼요. 지급 서비스가 연결되기 전에는 실제 송금이 진행되지 않아요.
              </div>
              <button className="button studio-submit-wide" type="submit">지급 요청 내용 저장</button>
            </form>
          </div>
        </Modal>
      )}

      {showPaySetup && <PaymentSetupModal onClose={() => setShowPaySetup(false)} />}
      {showPaymentSetup && <PaymentSetupModal onClose={() => setShowPaymentSetup(false)} />}
    </div>
  );
}
