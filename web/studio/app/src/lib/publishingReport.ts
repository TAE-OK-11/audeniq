// Report presentation: confirmed amounts and estimates stay separate.
import type { Royalty } from '../api/publishing';

const nextMonth = (p: string) => { const [y, m] = p.split('-').map(Number); return m === 12 ? `${y + 1}-01` : `${y}-${String(m + 1).padStart(2, '0')}`; };
const topBy = (rows: readonly Royalty[], key: (r: Royalty) => string) => {
  const m = new Map<string, number>();
  for (const r of rows) m.set(key(r), (m.get(key(r)) ?? 0) + r.net);
  return [...m.entries()].sort((a, b) => b[1] - a[1])[0]?.[0];
};
export function publishingReport(list: readonly Royalty[]) {
  if (!list.length) return null;
  const by = new Map<string, number>();
  for (const x of list) if (x.status !== 'estimated') by.set(x.period, (by.get(x.period) ?? 0) + x.net);
  const periods = [...(by.size ? by.keys() : new Set(list.map(x => x.period)))].sort();
  // 비는 달도 막대 자리는 남긴다 — 들어오지 않은 달이 그대로 보이게. 최근 여섯 달까지만
  const trend: { period: string; value: number }[] = [];
  for (let p = periods[0]; p <= periods[periods.length - 1]; p = nextMonth(p)) trend.push({ period: p, value: by.get(p) ?? 0 });
  const last = trend[trend.length - 1];
  const prev = trend[trend.length - 2];
  const lastRows = list.filter(x => x.period === last.period);
  return {
    period: last.period, amount: last.value, prev: prev?.value, trend: trend.slice(-6),
    estimated: list.filter(x => x.status === 'estimated').reduce((n, x) => n + x.net, 0),
    hasConfirmed: by.size > 0,
    topSong: topBy(lastRows.filter(x => x.status !== 'estimated'), x => x.workTitle), topSource: topBy(lastRows.filter(x => x.status !== 'estimated'), x => x.source),
  };
}
