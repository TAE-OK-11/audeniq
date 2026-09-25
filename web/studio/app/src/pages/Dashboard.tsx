import { Link } from 'react-router-dom';

const GRID_ITEMS = [
  {
    to: '/releases', title: '발매·곡 관리', desc: '발매 목록과 곡별 정보를 관리하세요',
    icon: <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="5" width="18" height="15" rx="3"/><path d="M7 5V3m5 2V3m5 2V3M7 10h10M7 14h6"/></svg>,
  },
  {
    to: '/reports', title: '음악 리포트', desc: '플랫폼별 재생과 수익 내역',
    icon: <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 19V5M4 19h17M8 15v-4m5 4V7m5 8V4"/></svg>,
  },
  {
    to: '/settlement', title: '정산·지급', desc: '정산 내역과 지급 요청',
    icon: <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m3 5 4.5 14 4.5-11L16.5 19 21 5M2 11h20M2 15h20"/></svg>,
  },
  {
    to: '/contracts', title: '계약서·권리', desc: '계약 상태와 증빙 서류',
    icon: <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M7 3h8l4 4v13a1 1 0 0 1-1 1H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2Z"/><path d="M15 3v5h4M9 12h6M9 16h6"/></svg>,
  },
];

export function Dashboard() {
  return (
    <>
      <div className="view-title">
        <div>
          <p className="eyebrow">AUDENIQ / STUDIO</p>
          <h1>서린님의 작업실</h1>
          <p>발매 현황과 확인할 작업을 한곳에서 살펴보세요.</p>
        </div>
        <Link className="button" to="/upload">새로운 발매</Link>
      </div>

      <div className="portal-hero">
        <div>
          <p className="eyebrow">NEW RELEASE</p>
          <h2>새로운 발매를<br />시작해 보세요.</h2>
          <p>음원과 커버아트, 크레딧을 등록하고 발매를 준비해 보세요.</p>
        </div>
        <Link className="button" to="/upload">발매 등록하기</Link>
      </div>

      <div className="dashboard-grid" aria-label="주요 업무">
        {GRID_ITEMS.map(item => (
          <Link key={item.to} className="service-item" to={item.to}>
            <span className="icon-chip">{item.icon}</span>
            <div>
              <h3>{item.title}</h3>
              <p>{item.desc}</p>
            </div>
          </Link>
        ))}
      </div>
    </>
  );
}
