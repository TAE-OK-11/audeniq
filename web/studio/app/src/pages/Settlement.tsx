// 정산·지급 — hero + 탭 구조의 단순화된 레이아웃
import { useState } from 'react';
import { Modal, useModalClose } from '../components/Modal';
import { useToast } from '../components/Toast';
import { useConfirm } from '../components/Confirm';
import { CountUp } from '../components/CountUp';
import { BankLogo } from '../components/BankLogo';
import { PaymentSetupModal } from '../components/PaymentSetupModal';
import { isPaymentRegistered, usePayment } from '../store/payment';
import { balance, payoutsStore, statementsStore } from '../store/settlement';
import { money, niceDate } from '../lib/format';
import { monthKey, todayStr } from '../lib/date';
import { uid } from '../lib/store';

const PLATFORMS = ['Spotify', 'Apple Music', 'YouTube Music', '멜론', '지니', 'FLO', '벅스', 'Amazon Music', 'TIDAL', 'Deezer', '기타'];

/** 정산 내역 직접 기록 폼 */
function StatementForm({ onSave }: { onSave: (s: { period: string; platform: string; amount: number; note: string }) => void }) {
  const close = useModalClose();
  const toast = useToast();
  const [period, setPeriod] = useState(monthKey(-1));
  const [platform, setPlatform] = useState(PLATFORMS[0]);
  const [amount, setAmount] = useState('');
  const [note, setNote] = useState('');
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    const amt = Number(amount.replace(/,/g, ''));
    if (!/^\d{4}-\d{2}$/.test(period)) { toast('정산 기간을 선택해 주세요.'); return; }
    if (!Number.isInteger(amt) || amt <= 0) { toast('정산 금액을 1원 이상 입력해 주세요.'); return; }
    onSave({ period, platform, amount: amt, note: note.trim() });
    close();
  };
  return (
    <form onSubmit={submit}>
      <p className="small muted">받은 정산서의 기간·플랫폼·금액을 기록하면 요청 가능 잔액에 반영돼요.</p>
      <div className="form-grid">
        <div className="field">
          <label htmlFor="stPeriod">정산 기간</label>
          <input id="stPeriod" type="month" required max={monthKey(0)} value={period} onChange={e => setPeriod(e.target.value)} />
        </div>
        <div className="field">
          <label htmlFor="stPlatform">플랫폼</label>
          <select id="stPlatform" value={platform} onChange={e => setPlatform(e.target.value)}>
            {PLATFORMS.map(p => <option key={p}>{p}</option>)}
          </select>
        </div>
      </div>
      <div className="field">
        <label htmlFor="stAmount">정산 금액 (원)</label>
        <input id="stAmount" inputMode="numeric" required placeholder="예: 24406" data-autofocus
          value={amount} onChange={e => setAmount(e.target.value.replace(/[^\d]/g, ''))} />
        {amount && <p className="help">{money(Number(amount))}</p>}
      </div>
      <div className="field">
        <label htmlFor="stNote">메모 (선택)</label>
        <input id="stNote" maxLength={200} value={note} onChange={e => setNote(e.target.value)} placeholder="정산서 번호 등" />
      </div>
      <button className="button studio-submit-wide" type="submit">정산 내역 기록</button>
    </form>
  );
}

export function Settlement() {
  const toast = useToast();
  const confirm = useConfirm();
  const payment = usePayment();
  const statements = statementsStore.use();
  const payouts = payoutsStore.use();
  const [tab, setTab] = useState<'statements' | 'payouts'>('statements');
  const [showPayout, setShowPayout] = useState(false);
  const [showStatement, setShowStatement] = useState(false);
  const [showPaymentSetup, setShowPaymentSetup] = useState(false);
  const [amount, setAmount] = useState('');
  const [payoutNote, setPayoutNote] = useState('');

  const paymentRegistered = isPaymentRegistered(payment);

  const { total, used, left } = balance(statements, payouts);

  const openPayoutModal = () => {
    if (!isPaymentRegistered(payment)) {
      toast('수익을 받을 정보를 먼저 등록해 주세요.');
      setShowPaymentSetup(true);
      return;
    }
    if (!left) { toast('지급을 요청할 수 있는 잔액이 없어요.'); return; }
    setAmount('');
    setPayoutNote('');
    setShowPayout(true);
  };

  const deleteStatement = async (id: string) => {
    const s = statements.find(x => x.id === id);
    if (!s) return;
    const remaining = balance(statements.filter(x => x.id !== id), payouts);
    if (remaining.total < remaining.used) {
      toast('지급 요청 합계보다 정산액이 적어져서 삭제할 수 없어요. 요청 기록을 먼저 정리해 주세요.');
      return;
    }
    if (!(await confirm({ title: '정산 내역을 삭제할까요?', message: `${s.period} · ${s.platform} · ${money(s.amount)}`, confirmLabel: '삭제', danger: true }))) return;
    statementsStore.set(ss => ss.filter(x => x.id !== id));
    toast('정산 내역을 삭제했어요.');
  };

  const deletePayout = async (id: string) => {
    const p = payouts.find(x => x.id === id);
    if (!p) return;
    if (!(await confirm({ title: '지급 요청 기록을 삭제할까요?', message: `${niceDate(p.created)} · ${money(p.amount)}`, confirmLabel: '삭제', danger: true }))) return;
    payoutsStore.set(ps => ps.filter(x => x.id !== id));
    toast('지급 요청 기록을 삭제했어요.');
  };

  const submitPayout = (e: React.FormEvent) => {
    e.preventDefault();
    const amt = Number(amount);
    if (!Number.isInteger(amt) || amt <= 0 || amt > left) {
      toast('요청 가능 금액 안에서 입력해 주세요.');
      return;
    }
    payoutsStore.set(ps => [...ps, { id: uid('p'), amount: amt, note: payoutNote.trim(), created: todayStr(), status: 'recorded' }]);
    setShowPayout(false);
    setTab('payouts');
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
            <strong className="settle-hero-amount"><CountUp value={left} format={money} /></strong>
            <p className="settle-hero-sub">
              기록한 정산액 {money(total)} · 지급 요청 합계 {money(used)}
            </p>
          </div>
          <div className="row-actions">
            <button type="button" className="button secondary" onClick={() => setShowStatement(true)}>정산 기록</button>
            <button type="button" className="button" onClick={openPayoutModal}>수익 받기</button>
          </div>
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

      <div className="tabs aq-tabs" role="tablist" aria-label="정산 내역 구분">
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
        <div id="statementList" className="aq-tab-panel" key="st">
          {orderedStatements.length ? (
            <div className="aq-catalog-cards aq-stagger">
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
              <button type="button" className="button" onClick={() => setShowStatement(true)}>정산 내역 기록하기</button>
            </div>
          )}
        </div>
      ) : (
        <div id="payoutList" className="aq-tab-panel" key="po">
          {payouts.length ? (
            <div className="aq-catalog-cards aq-stagger">
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

      {showStatement && (
        <Modal title="정산 내역 기록" onClose={() => setShowStatement(false)}>
          <StatementForm
            onSave={st => {
              statementsStore.set(ss => [...ss, { id: uid('st'), ...st, created: todayStr() }]);
              setTab('statements');
              toast('정산 내역을 기록했어요.');
            }}
          />
        </Modal>
      )}

      {showPaymentSetup && <PaymentSetupModal onClose={() => setShowPaymentSetup(false)} />}
    </div>
  );
}
