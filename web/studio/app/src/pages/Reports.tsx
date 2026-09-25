import { useState } from 'react';
import { useToast } from '../components/Toast';

const PERIODS = [
  { value: 'all', label: '전체 기간' },
  { value: 'month', label: '이번 달' },
  { value: 'prev', label: '지난달' },
];

interface ReportRow {
  period: string; platform: string; release: string; track: string; plays: number; revenue: number;
}

const MOCK_ROWS: ReportRow[] = [
  { period: '2026-09', platform: 'Spotify', release: '첫 번째 싱글', track: '첫 번째 싱글', plays: 12480, revenue: 10736 },
  { period: '2026-09', platform: 'Apple Music', release: '첫 번째 싱글', track: '첫 번째 싱글', plays: 8216, revenue: 9202 },
  { period: '2026-09', platform: 'Melon', release: '여름 EP', track: '파도', plays: 5934, revenue: 4391 },
  { period: '2026-09', platform: 'YouTube Music', release: '여름 EP', track: '전체', plays: 4102, revenue: 2789 },
  { period: '2026-08', platform: 'Spotify', release: '첫 번째 싱글', track: '첫 번째 싱글', plays: 9870, revenue: 8492 },
  { period: '2026-08', platform: 'Melon', release: '첫 번째 싱글', track: '전체', plays: 4210, revenue: 3115 },
];

function money(n: number): string {
  return new Intl.NumberFormat('ko-KR', { style: 'currency', currency: 'KRW', maximumFractionDigits: 0 }).format(n || 0);
}

function num(n: number): string {
  return new Intl.NumberFormat('ko-KR').format(n || 0);
}

function makeChart(rows: ReportRow[]) {
  const buckets = new Map<string, number>();
  for (const r of rows) {
    const k = r.period.slice(0, 7) || '기타';
    buckets.set(k, (buckets.get(k) || 0) + Number(r.revenue || 0));
  }
  const vals = [...buckets.entries()].sort((a, b) => a[0].localeCompare(b[0])).slice(-8);
  if (!vals.length) {
    return (
      <div className="empty-graph">
        아직 수익 리포트가 없어요.<br />정산이 반영되면 이곳에서 확인할 수 있어요.
      </div>
    );
  }
  const high = Math.max(1, ...vals.map(x => x[1]));
  return (
    <>
      <div className="report-chart" role="img" aria-label="기간별 수익 막대그래프">
        {vals.map(([k, v]) => (
          <div
            key={k}
            className="report-bar"
            style={{ ['--h' as string]: `${Math.max(2, Math.round((v / high) * 100))}%` }}
            title={`${k} · ${money(v)}`}
          />
        ))}
      </div>
      <div className="report-axis">
        <span>{vals[0][0]}</span>
        <span>{vals[vals.length - 1][0]}</span>
      </div>
    </>
  );
}

export function Reports() {
  const toast = useToast();
  const [period, setPeriod] = useState('all');
  const [release, setRelease] = useState('all');

  const thisMonth = '2026-09';
  const lastMonth = '2026-08';

  const filtered = MOCK_ROWS.filter(r => {
    if (period === 'month' && r.period !== thisMonth) return false;
    if (period === 'prev' && r.period !== lastMonth) return false;
    if (release !== 'all' && r.release !== release) return false;
    return true;
  });

  const totalRevenue = filtered.reduce((n, r) => n + r.revenue, 0);
  const totalPlays = filtered.reduce((n, r) => n + r.plays, 0);
  const platforms = new Set(filtered.map(r => r.platform)).size;

  const releases = [...new Set(MOCK_ROWS.map(r => r.release))];

  const exportCsv = () => {
    if (!filtered.length) { toast('내보낼 리포트가 없어요.'); return; }
    const cell = (v: string | number) => `"${String(v ?? '').replace(/"/g, '""')}"`;
    const header = 'period,platform,releaseTitle,track,plays,revenue';
    const lines = filtered.map(r =>
      [r.period, r.platform, r.release, r.track, r.plays, r.revenue].map(cell).join(','));
    const csv = '\uFEFF' + [header, ...lines].join('\r\n');
    const blob = new Blob([csv], { type: 'text/csv;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = 'AUDENIQ_report.csv';
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    setTimeout(() => URL.revokeObjectURL(url), 1000);
    toast('리포트를 CSV로 내보냈어요.');
  };

  return (
    <>
      <div className="view-title">
        <div>
          <p className="eyebrow">INSIGHTS</p>
          <h1>음악 리포트</h1>
          <p>플랫폼별 실적을 기간과 곡별로 확인해 보세요.</p>
        </div>
        <button type="button" className="button secondary" onClick={exportCsv}>CSV 내보내기 ↗</button>
      </div>

      <div className="report-tools">
        <select aria-label="리포트 기간" value={period} onChange={e => setPeriod(e.target.value)}>
          {PERIODS.map(p => <option key={p.value} value={p.value}>{p.label}</option>)}
        </select>
        <select aria-label="발매별 필터" value={release} onChange={e => setRelease(e.target.value)}>
          <option value="all">전체 발매</option>
          {releases.map(r => <option key={r} value={r}>{r}</option>)}
        </select>
      </div>

      <div className="stat-grid">
        <div className="surface white stat-card"><small>집계 수익</small><strong>{money(totalRevenue)}</strong></div>
        <div className="surface white stat-card"><small>재생 수</small><strong>{num(totalPlays)}</strong></div>
        <div className="surface white stat-card"><small>플랫폼</small><strong>{platforms}</strong></div>
      </div>

      <section className="surface" aria-labelledby="reportGraphTitle">
        <div className="section-top">
          <h2 id="reportGraphTitle">기간별 수익</h2>
          <span className="muted small">기간별 재생·수익 내역</span>
        </div>
        <div id="reportGraph">{makeChart(filtered)}</div>
      </section>

      <div className="section-top"><h2>플랫폼별 상세 내역</h2></div>
      {filtered.length ? (
        <div className="scroll-x">
          <table className="data-table">
            <thead>
              <tr>
                <th>기간</th><th>플랫폼</th><th>발매 / 곡</th>
                <th className="num">재생</th><th className="num">수익</th>
              </tr>
            </thead>
            <tbody>
              {filtered.map((r, i) => (
                <tr key={i}>
                  <td>{r.period}</td>
                  <td>{r.platform}</td>
                  <td>{r.release || '—'} / {r.track || '전체'}</td>
                  <td className="num">{num(r.plays)}</td>
                  <td className="num">{money(r.revenue)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : (
        <div className="empty-page">
          <h2>아직 가져온 실적이 없어요.</h2>
          <p>CSV 파일을 가져오면 재생·수익 내역과 기간별 그래프가 여기에 표시돼요.</p>
        </div>
      )}
    </>
  );
}
