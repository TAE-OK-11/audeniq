import { useState } from 'react';
import { useNavigate } from 'react-router-dom';

const STATEMENTS = [
  { id: 'st1', title: '2026년 8월 정산', amount: '₩24,406', status: '확정', date: '2026-09-10' },
  { id: 'st2', title: '2026년 7월 정산', amount: '₩18,920', status: '지급 완료', date: '2026-08-12' },
  { id: 'st3', title: '2026년 6월 정산', amount: '₩15,304', status: '지급 완료', date: '2026-07-11' },
];

const PAYOUTS = [
  { id: 'p1', title: '2026년 7월 정산 지급', amount: '₩18,920', status: '지급 완료', date: '2026-08-15' },
  { id: 'p2', title: '2026년 6월 정산 지급', amount: '₩15,304', status: '지급 완료', date: '2026-07-14' },
];

export function Settlement() {
  const nav = useNavigate();
  const [requested, setRequested] = useState(false);

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

      <div className="stat-grid">
        <div className="stat-card"><small>확정 정산액</small><strong>₩58,630</strong></div>
        <div className="stat-card"><small>지급 완료</small><strong>₩34,224</strong></div>
        <div className="stat-card"><small>지급 가능</small><strong>₩24,406</strong></div>
      </div>

      <section className="studio-payout-surface" aria-labelledby="payoutHeading">
        <div>
          <span className="eyebrow">PAYOUT</span>
          <h2 id="payoutHeading">수익을 받아보세요.</h2>
          <p>지급 가능한 내역과 수령 정보를 확인한 뒤 요청할 수 있어요.</p>
        </div>
        <div className="studio-payout-actions" style={{ display: 'flex', alignItems: 'center', gap: 16, flexWrap: 'wrap' }}>
          <span className="studio-available">₩24,406</span>
          <button
            type="button" className="button"
            onClick={() => setRequested(true)}
            disabled={requested}
          >
            {requested ? '요청됨' : '수익 받기'}
          </button>
        </div>
      </section>
      {requested && (
        <div className="notice success">지급 요청이 접수됐어요. (테스트 모드)</div>
      )}

      <div className="notice">
        확정된 정산 금액과 지급 진행 상황을 확인할 수 있어요. 수령 금액은 정산이 확정된 뒤 안내해 드려요.
      </div>

      <div className="section-top"><h2>정산 내역</h2></div>
      <div className="data-list">
        {STATEMENTS.map(s => (
          <div key={s.id} className="track-row">
            <div>
              <span className="row-name">{s.title}</span>
              <span className="row-sub">{s.date}</span>
            </div>
            <div><span className="row-name">{s.amount}</span></div>
            <div className="row-end">
              <span className={`status-chip ${s.status === '확정' ? 'ready' : 'live'}`}>{s.status}</span>
            </div>
          </div>
        ))}
      </div>

      <div className="section-top"><h2>지급 요청 기록</h2></div>
      <div className="data-list">
        {PAYOUTS.map(p => (
          <div key={p.id} className="track-row">
            <div>
              <span className="row-name">{p.title}</span>
              <span className="row-sub">{p.date}</span>
            </div>
            <div><span className="row-name">{p.amount}</span></div>
            <div className="row-end">
              <span className="status-chip live">{p.status}</span>
            </div>
          </div>
        ))}
      </div>
    </>
  );
}
