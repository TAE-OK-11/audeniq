import { useState } from 'react';

const PERIODS = [
  { value: 'all', label: '전체 기간' },
  { value: 'month', label: '이번 달' },
  { value: 'prev', label: '지난달' },
];

const RELEASES = ['첫 번째 싱글', '여름 EP', '데모 트랙'];

const PLATFORMS = [
  { name: 'Spotify', plays: '12,480', revenue: '₩10,736' },
  { name: 'Apple Music', plays: '8,216', revenue: '₩9,202' },
  { name: 'Melon', plays: '5,934', revenue: '₩4,391' },
  { name: 'YouTube Music', plays: '4,102', revenue: '₩2,789' },
];

const BARS = [42, 68, 55, 90, 74, 100];

export function Reports() {
  const [period, setPeriod] = useState('all');
  const [release, setRelease] = useState('all');

  return (
    <>
      <div className="view-title">
        <div>
          <p className="eyebrow">INSIGHTS</p>
          <h1>음악 리포트</h1>
          <p>플랫폼별 실적을 기간과 곡별로 확인해 보세요.</p>
        </div>
        <button type="button" className="button secondary">CSV 내보내기 ↗</button>
      </div>

      <div className="report-tools">
        <select aria-label="리포트 기간" value={period} onChange={e => setPeriod(e.target.value)}>
          {PERIODS.map(p => <option key={p.value} value={p.value}>{p.label}</option>)}
        </select>
        <select aria-label="발매별 필터" value={release} onChange={e => setRelease(e.target.value)}>
          <option value="all">전체 발매</option>
          {RELEASES.map(r => <option key={r} value={r}>{r}</option>)}
        </select>
      </div>

      <div className="stat-grid">
        <div className="stat-card"><small>총 재생</small><strong>30,732</strong></div>
        <div className="stat-card"><small>총 수익</small><strong>₩27,118</strong></div>
        <div className="stat-card"><small>정산 예정</small><strong>₩24,406</strong></div>
      </div>

      <section className="surface" aria-labelledby="reportGraphTitle">
        <div className="section-top">
          <h2 id="reportGraphTitle">기간별 수익</h2>
          <span className="muted small">기간별 재생·수익 내역</span>
        </div>
        <div style={{ display: 'flex', alignItems: 'flex-end', gap: 10, height: 140, paddingTop: 8 }} aria-hidden="true">
          {BARS.map((h, i) => (
            <div
              key={i}
              style={{
                flex: 1, height: `${h}%`, minHeight: 8,
                background: i === BARS.length - 1 ? '#3B63F3' : '#E3EAFB',
                borderRadius: 8,
              }}
            />
          ))}
        </div>
        <p className="small muted" style={{ marginTop: 10 }}>테스트 데이터 기준 (mock)</p>
      </section>

      <div className="section-top"><h2>플랫폼별 상세 내역</h2></div>
      <div className="data-list">
        {PLATFORMS.map(p => (
          <div key={p.name} className="track-row">
            <div><span className="row-name">{p.name}</span></div>
            <div><span className="row-sub">{p.plays}회 재생</span></div>
            <div className="row-end"><span className="row-name">{p.revenue}</span></div>
          </div>
        ))}
      </div>
    </>
  );
}
