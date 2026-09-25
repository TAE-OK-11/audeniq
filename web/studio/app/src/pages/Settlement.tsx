import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Modal } from '../components/Modal';
import { useToast } from '../components/Toast';

interface Statement { id: string; period: string; platform: string; amount: number; note: string; created: string }
interface Payout { id: string; amount: number; note: string; created: string; status: string }

const INITIAL_STATEMENTS: Statement[] = [
  { id: 'st1', period: '2026-08', platform: 'Spotify', amount: 24406, note: '', created: '2026-09-10' },
  { id: 'st2', period: '2026-07', platform: 'Apple Music', amount: 18920, note: '', created: '2026-08-12' },
];

const INITIAL_PAYOUTS: Payout[] = [
  { id: 'p1', amount: 18920, note: '', created: '2026-08-15', status: 'recorded' },
];

function money(n: number): string {
  return new Intl.NumberFormat('ko-KR', { style: 'currency', currency: 'KRW', maximumFractionDigits: 0 }).format(n || 0);
}

export function Settlement() {
  const nav = useNavigate();
  const toast = useToast();
  const [statements] = useState<Statement[]>(INITIAL_STATEMENTS);
  const [payouts, setPayouts] = useState<Payout[]>(INITIAL_PAYOUTS);
  const [showPayout, setShowPayout] = useState(false);
  const [amount, setAmount] = useState('');
  const [payoutNote, setPayoutNote] = useState('');

  const total = statements.reduce((n, s) => n + Number(s.amount || 0), 0);
  const used = payouts.reduce((n, p) => n + Number(p.amount || 0), 0);
  const left = Math.max(0, total - used);

  const openPayoutModal = () => {
    if (!left) { toast('지급을 요청할 수 있는 잔액이 없어요.'); return; }
    setAmount('');
    setPayoutNote('');
    setShowPayout(true);
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

  return (
    <>
      <div className="view-title">
        <div>
          <p className="eyebrow">ROYALTIES &amp; PAYOUTS</p>
          <h1>정산·지급</h1>
          <p>확정된 정산 금액과 지급 요청 내역을 확인해 보세요.</p>
        </div>
        <button type="button" className="button secondary" onClick={() => nav('/profile')}>
          정산 정보 관리 ↗
        </button>
      </div>

      <div className="stat-grid" id="settlementStats">
        <div className="surface white stat-card"><small>확정된 정산 금액</small><strong>{money(total)}</strong></div>
        <div className="surface white stat-card"><small>수령 가능 금액</small><strong>{money(left)}</strong></div>
        <div className="surface white stat-card"><small>지급 요청 금액</small><strong>{money(used)}</strong></div>
      </div>

      <section className="studio-payout-surface" aria-labelledby="payoutHeading">
        <div>
          <span className="eyebrow">PAYOUT</span>
          <h2 id="payoutHeading">수익을 받아보세요.</h2>
          <p>지급 가능한 내역과 수령 정보를 확인한 뒤 요청할 수 있어요.</p>
        </div>
        <div className="studio-payout-actions" style={{ display: 'flex', alignItems: 'center', gap: 16, flexWrap: 'wrap' }}>
          <span className="studio-available">{money(left)}</span>
          <button type="button" className="button" onClick={openPayoutModal}>수익 받기</button>
        </div>
      </section>

      <div className="notice">
        확정된 정산 금액과 지급 진행 상황을 확인할 수 있어요. 수령 금액은 정산이 확정된 뒤 안내해 드려요.
      </div>

      <div className="section-top">
        <h2>정산 내역</h2>
      </div>
      <div className="data-list">
        {statements.length ? statements.map(s => (
          <div key={s.id} className="statement-row">
            <span className="document-icon">₩</span>
            <div>
              <span className="row-name">{s.period} · {s.platform}</span>
              <span className="row-sub">{s.note || '최종 지급 예정 금액'} · {s.created}</span>
            </div>
            <div className="row-end">
              <strong>{money(s.amount)}</strong>
              <span className="status-chip live">확정</span>
            </div>
          </div>
        )) : (
          <div className="empty-note">확정된 정산 내역이 없어요. 정산이 확정되면 여기에서 확인할 수 있어요.</div>
        )}
      </div>

      <div className="section-top"><h2>지급 요청 기록</h2></div>
      <div className="data-list">
        {payouts.length ? payouts.map(p => (
          <div key={p.id} className="statement-row">
            <span className="document-icon">₩</span>
            <div>
              <span className="row-name">{money(p.amount)} · 수익 지급 요청</span>
              <span className="row-sub">{p.created} · {p.status === 'recorded' ? '접수 전 · 신청 내용 보관' : '진행 중'}</span>
            </div>
          </div>
        )) : (
          <div className="empty-note">지급 요청 내역이 없어요. 수령 가능 금액을 확인하고 지급을 요청할 수 있어요.</div>
        )}
      </div>

      {showPayout && (
        <Modal title="수익 지급 요청" onClose={() => setShowPayout(false)}>
          <div className="studio-payout-form">
            <p className="eyebrow">지급 요청 가능 금액</p>
            <strong className="studio-payout-number">{money(left)}</strong>
            <p className="small muted">원하는 금액을 입력하고 받으실 계좌를 확인해 주세요.</p>
            <form onSubmit={submitPayout}>
              <div className="field">
                <label htmlFor="pAmount">받을 금액 (원)</label>
                <div className="studio-amount-field">
                  <input
                    id="pAmount" inputMode="numeric" type="number" min={1} max={left} step={1}
                    required placeholder="금액을 입력해 주세요"
                    value={amount} onChange={e => setAmount(e.target.value)}
                  />
                  <button type="button" className="link-btn" onClick={() => setAmount(String(left))}>전액</button>
                </div>
                <p className="help">요청 가능 금액을 초과할 수 없어요.</p>
              </div>
              <div className="studio-receive-account">
                <span className="document-icon" aria-hidden="true">₩</span>
                <div>
                  <strong>국민은행</strong>
                  <p className="small muted">서린 · •••• 1234</p>
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


    </>
  );
}
