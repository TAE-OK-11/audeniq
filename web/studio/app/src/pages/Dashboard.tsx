import { Link } from 'react-router-dom';

export function Dashboard() {
  return (
    <>
      <p className="eyebrow">STUDIO</p>
      <h1 className="section-title">대시보드</h1>
      <p className="section-sub">발매 현황을 한눈에 확인하세요.</p>
      <div className="stats-grid">
        <div className="stat-card">
          <div className="stat-kicker">RELEASES</div>
          <div className="stat-value">0</div>
          <div className="stat-label">발매 중</div>
        </div>
        <div className="stat-card">
          <div className="stat-kicker">LIVE</div>
          <div className="stat-value">0</div>
          <div className="stat-label">배포 완료</div>
        </div>
        <div className="stat-card">
          <div className="stat-kicker">ACTION</div>
          <div className="stat-value">0</div>
          <div className="stat-label">수정 필요</div>
        </div>
      </div>

      <div className="card">
        <h3>시작하기</h3>
        <p style={{ color: 'var(--muted)', fontSize: 15 }}>
          첫 발매를 등록하고 음악을 세상에 내놓으세요.
        </p>
        <div style={{ marginTop: 16, display: 'flex', gap: 12 }}>
          <Link to="/upload" className="button">새 발매 만들기</Link>
          <Link to="/releases" className="button button-secondary">발매 목록</Link>
        </div>
      </div>
    </>
  );
}
