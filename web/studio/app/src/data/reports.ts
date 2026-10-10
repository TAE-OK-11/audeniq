// 음악 리포트 목 데이터 — Reports 화면과 홈 요약이 같은 원천을 쓴다.
export interface ReportRow {
  period: string; platform: string; release: string; track: string; plays: number; revenue: number;
}

export const REPORT_ROWS: ReportRow[] = [
  { period: '2026-09', platform: 'Spotify', release: '첫 번째 싱글', track: '첫 번째 싱글', plays: 12480, revenue: 10736 },
  { period: '2026-09', platform: 'Apple Music', release: '첫 번째 싱글', track: '첫 번째 싱글', plays: 8216, revenue: 9202 },
  { period: '2026-09', platform: 'Melon', release: '여름 EP', track: '파도', plays: 5934, revenue: 4391 },
  { period: '2026-09', platform: 'YouTube Music', release: '여름 EP', track: '전체', plays: 4102, revenue: 2789 },
  { period: '2026-08', platform: 'Spotify', release: '첫 번째 싱글', track: '첫 번째 싱글', plays: 9870, revenue: 8492 },
  { period: '2026-08', platform: 'Melon', release: '첫 번째 싱글', track: '전체', plays: 4210, revenue: 3115 },
  { period: '2026-07', platform: 'Spotify', release: '첫 번째 싱글', track: '첫 번째 싱글', plays: 7020, revenue: 6120 },
  { period: '2026-07', platform: 'Apple Music', release: '첫 번째 싱글', track: '첫 번째 싱글', plays: 3380, revenue: 3890 },
];

/** 데이터에 있는 기간 목록 (최신순) */
export function periods(rows: ReportRow[] = REPORT_ROWS): string[] {
  return [...new Set(rows.map(r => r.period))].sort().reverse();
}

export function sum(rows: ReportRow[], key: 'plays' | 'revenue'): number {
  return rows.reduce((n, r) => n + (r[key] || 0), 0);
}

export function topBy(rows: ReportRow[], key: 'platform' | 'track'): string {
  const map = new Map<string, number>();
  for (const r of rows) {
    if (key === 'track' && r.track === '전체') continue;
    map.set(r[key], (map.get(r[key]) || 0) + r.revenue);
  }
  let best = ''; let bestV = -1;
  for (const [k, v] of map) if (v > bestV) { best = k; bestV = v; }
  return best;
}

/** 최신 월 요약 — 홈 카드용 */
export function latestSummary(rows: ReportRow[] = REPORT_ROWS) {
  const [cur, prev] = periods(rows);
  const curRows = rows.filter(r => r.period === cur);
  const prevRows = rows.filter(r => r.period === prev);
  const revenue = sum(curRows, 'revenue');
  const prevRevenue = sum(prevRows, 'revenue');
  return {
    period: cur,
    revenue,
    plays: sum(curRows, 'plays'),
    change: prevRevenue > 0 ? ((revenue - prevRevenue) / prevRevenue) * 100 : null,
    topTrack: topBy(curRows, 'track'),
    topPlatform: topBy(curRows, 'platform'),
    trend: periods(rows).slice(0, 6).reverse().map(p => ({ period: p, revenue: sum(rows.filter(r => r.period === p), 'revenue') })),
  };
}

export function periodLabel(p: string): string {
  const m = /^(\d{4})-(\d{2})$/.exec(p);
  return m ? `${m[1]}년 ${Number(m[2])}월` : p;
}
