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
  const [statements, setStatements] = useState<Statement[]>(INITIAL_STATEMENTS);
  const [payouts, setPayouts] = useState<Payout[]>(INITIAL_PAYOUTS);
  const [showPayout, setShowPayout] = useState(false);
  const [showStatement, setShowStatement] = useState(false);
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

  const submitStatement = (e: React.FormEvent) => {
    e.preventDefault();
    const form = e.target as HTMLFormElement;
    const data = new FormData(form);
    const period = String(data.get('sPeriod') || '');
    const platform = String(data.get('sPlatform') || '').trim();
    const amt = Number(data.get('sAmount') || 0);
    const note = String(data.get('sNote') || '').trim();
    if (!period || !platform || !amt) { toast('필수 항목을 입력해 주세요.'); return; }
    setStatements(ss => [...ss, { id: 'st' + Date.now(), period, platform, amount: amt, note, created: new Date().toISOString().slice(0, 10) }]);
    setShowStatement(false);
    toast('정산 내역을 저장했어요.');
  };

  const deleteStatement = (id: string) => {
    if (!window.confirm('선택한 내역을 삭제할까요?')) return;
    setStatements(ss => ss.filter(s => s.id !== id));
  };

  const deletePayout = (id: string) => {
    if (!window.confirm('선택한 내역을 삭제할까요?')) return;
    setPayouts(ps => ps.filter(p => p.id !== id));
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
        <div className="surface white stat-card"><small>기록한 정산액</small><strong>{money(total)}</strong></div>
        <div className="surface white stat-card"><small>요청 전 잔액</small><strong>{money(left)}</strong></div>
        <div className="surface white stat-card"><small>지급 요청 기록 합계</small><strong>{money(used)}</strong></div>
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
        정산 내역과 지급 요청 기록을 확인해 주세요. 실제 송금은 지급 서비스 연결 후 진행돼요.
      </div>

      <div className="section-top">
        <h2>정산 내역</h2>
        <div className="row-actions">
          <button type="button" className="button ghost" onClick={() => setShowStatement(true)}>정산 내역 기록</button>
        </div>
      </div>
      <div className="data-list">
        {statements.length ? statements.map(s => (
          <div key={s.id} className="statement-row">
            <span className="document-icon">₩</span>
            <div>
              <span className="row-name">{s.period} · {s.platform}</span>
              <span className="row-sub">{s.note || '수기 등록 정산 내역'} · {s.created}</span>
            </div>
            <div className="row-end">
              <strong>{money(s.amount)}</strong>
              <button type="button" className="link-btn" onClick={() => deleteStatement(s.id)} aria-label="정산 내역 삭제">×</button>
            </div>
          </div>
        )) : (
          <div className="empty-note">기록된 정산 내역이 없어요.</div>
        )}
      </div>

      <div className="section-top"><h2>지급 요청 기록</h2></div>
      <div className="data-list">
        {payouts.length ? payouts.map(p => (
          <div key={p.id} className="statement-row">
            <span className="document-icon">↗</span>
            <div>
              <span className="row-name">{money(p.amount)} · 지급 요청 기록</span>
              <span className="row-sub">{p.created} · {p.note || '현재 작업 공간에만 기록됨'}</span>
            </div>
            <div className="row-end">
              <span className="status-chip ready">전송 전</span>
              <button type="button" className="link-btn" onClick={() => deletePayout(p.id)} aria-label="요청 기록 삭제">×</button>
            </div>
          </div>
        )) : (
          <div className="empty-note">지급 요청 기록이 없어요.</div>
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

      {showStatement && (
        <Modal title="정산 내역 기록" onClose={() => setShowStatement(false)}>
          <p className="small muted">받은 정산서를 확인한 뒤 직접 입력해 주세요. 기록된 금액은 AUDENIQ에서 확정한 금액이 아니에요.</p>
          <form onSubmit={submitStatement} style={{ marginTop: 16 }}>
            <div className="field">
              <label htmlFor="sPeriod">정산 기간 (월)</label>
              <input id="sPeriod" name="sPeriod" type="month" required />
            </div>
            <div className="field">
              <label htmlFor="sPlatform">지급 플랫폼 / 정산 출처</label>
              <input id="sPlatform" name="sPlatform" maxLength={100} required placeholder="예: Spotify" />
            </div>
            <div className="field">
              <label htmlFor="sAmount">정산 금액 (원)</label>
              <input id="sAmount" name="sAmount" type="number" min={0} step={1} required placeholder="0" />
            </div>
            <div className="field">
              <label htmlFor="sNote">메모</label>
              <input id="sNote" name="sNote" maxLength={200} placeholder="정산서 번호 등" />
            </div>
            <button className="button" type="submit">내역 저장</button>
          </form>
        </Modal>
      )}
    </>
  );
}
