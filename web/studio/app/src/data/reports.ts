// 음악 리포트 목 데이터 — Reports 화면과 홈 요약이 같은 원천을 쓴다.
export interface ReportRow {
  period: string; platform: string; release: string; track: string; plays: number; revenue: number;
  /** 실서버: 매칭된 발매 ID (발매 상세의 성과에서 이 발매 행만 고른다) */
  releaseId?: string;
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

/** 가장 큰 곡·플랫폼 — 기본은 수익 기준, by='plays'면 들은 횟수 기준 */
export function topBy(rows: ReportRow[], key: 'platform' | 'track', by: 'plays' | 'revenue' = 'revenue'): string {
  const map = new Map<string, number>();
  for (const r of rows) {
    if (key === 'track' && r.track === '전체') continue;
    map.set(r[key], (map.get(r[key]) || 0) + r[by]);
  }
  let best = ''; let bestV = -1;
  for (const [k, v] of map) if (v > bestV) { best = k; bestV = v; }
  return best;
}

/** 'YYYY-MM'의 바로 앞 달 (형식이 아니면 빈 값) */
export function prevMonth(p: string): string {
  const m = /^(\d{4})-(\d{2})/.exec(p);
  if (!m) return '';
  const y = +m[1], mo = +m[2];
  return mo === 1 ? `${y - 1}-12` : `${y}-${String(mo - 1).padStart(2, '0')}`;
}

/** 최신 월 요약 — 홈 카드용 (비교는 달력상 바로 앞 달과) */
export function latestSummary(rows: ReportRow[] = REPORT_ROWS) {
  const [cur = ''] = periods(rows);
  const prev = prevMonth(cur);
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

/** 한 달 재생 수의 기념할 만한 고비 */
const PLAY_MILESTONES = [1000, 5000, 10000, 30000, 50000, 100000, 300000, 500000, 1000000];

/** 1,000 · 5,000 · 1만 · 3만 · 100만 */
export function countLabel(n: number): string {
  return n >= 10000 ? `${n / 10000}만` : n.toLocaleString('ko-KR');
}

/**
 * 홈 '음악 리포트' — 돈이 아니라 얼마나 들렸는지. 큰 숫자는 늘 쌓인 재생 수(줄어든 달에도 성과),
 * 지난달은 비교용으로만, 좋은 소식(처음 넘은 고비 · 가장 많이 들은 달)이 있을 때만 한 줄.
 * 수익은 정산·리포트 화면에서 본다 (홈에 금액 두 개를 나란히 두지 않는다).
 */
export function listeningSummary(rows: ReportRow[] = REPORT_ROWS) {
  const all = periods(rows);
  const [cur = ''] = all;
  const playsOf = (p: string) => sum(rows.filter(r => r.period === p), 'plays');
  const prev = prevMonth(cur);
  const curRows = rows.filter(r => r.period === cur);
  const plays = playsOf(cur);
  const earlier = all.slice(1).map(playsOf);
  const bestBefore = earlier.length ? Math.max(...earlier) : 0;
  const crossed = PLAY_MILESTONES.filter(t => plays >= t && bestBefore < t).pop();
  const highlight = !plays ? null
    : crossed && earlier.length ? `처음으로 한 달 ${countLabel(crossed)} 번을 넘었어요`
    : earlier.length && plays > bestBefore ? '가장 많이 들은 달이에요'
    : null;
  return {
    period: cur,
    plays,
    prevPlays: all.includes(prev) ? playsOf(prev) : null,
    highlight,
    topTrack: topBy(curRows, 'track', 'plays'),
    topPlatform: topBy(curRows, 'platform', 'plays'),
    trend: all.slice(0, 6).reverse().map(p => ({ period: p, value: playsOf(p) })),
  };
}

export function periodLabel(p: string): string {
  const m = /^(\d{4})-(\d{2})$/.exec(p);
  return m ? `${m[1]}년 ${Number(m[2])}월` : p;
}
