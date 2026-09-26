import { useMemo, useState } from 'react';
import { useToast } from '../components/Toast';
import { CountUp } from '../components/CountUp';
import { money, num } from '../lib/format';
import { todayStr } from '../lib/date';
import { useGrowOnView, type CSSVarStyle } from '../hooks/useAnimations';
import { REPORT_ROWS as MOCK_ROWS, periodLabel as monthLabel, periods, type ReportRow } from '../data/reports';

const PERIODS = [
  { value: 'all', label: '전체 기간' },
  { value: 'month', label: '최근 달' },
  { value: 'prev', label: '그 전 달' },
];

function Chart({ rows }: { rows: ReportRow[] }) {
  const chartRef = useGrowOnView<HTMLDivElement>();
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
  const barStyle = (v: number): CSSVarStyle => ({
    '--h': `${Math.max(2, Math.round((v / high) * 100))}%`,
  });
  return (
    <>
      <div className="report-chart" ref={chartRef} role="img" aria-label="기간별 수익 막대그래프">
        {vals.map(([k, v]) => (
          <div
            key={k}
            className="report-bar"
            style={barStyle(v)}
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

  const [thisMonth = '', lastMonth = ''] = periods(MOCK_ROWS);

  const filtered = useMemo(() => MOCK_ROWS.filter(r => {
    if (period === 'month' && r.period !== thisMonth) return false;
    if (period === 'prev' && r.period !== lastMonth) return false;
    if (release !== 'all' && r.release !== release) return false;
    return true;
  }), [period, release, thisMonth, lastMonth]);

  const totalRevenue = filtered.reduce((n, r) => n + r.revenue, 0);
  const totalPlays = filtered.reduce((n, r) => n + r.plays, 0);
  const platforms = new Set(filtered.map(r => r.platform)).size;

  const byPlatform = useMemo(() => {
    const map = new Map<string, { plays: number; revenue: number }>();
    for (const r of filtered) {
      const cur = map.get(r.platform) || { plays: 0, revenue: 0 };
      cur.plays += r.plays;
      cur.revenue += r.revenue;
      map.set(r.platform, cur);
    }
    return [...map.entries()].sort((a, b) => b[1].revenue - a[1].revenue);
  }, [filtered]);
  const maxPlatformRevenue = Math.max(1, ...byPlatform.map(([, v]) => v.revenue));

  const releases = [...new Set(MOCK_ROWS.map(r => r.release))];

  const periodLabel = period === 'month' ? monthLabel(thisMonth) : period === 'prev' ? monthLabel(lastMonth) : '전체 기간';

  // 전월 대비 인사이트 (발매 필터는 반영, 기간 필터와 무관하게 두 달 비교)
  const insight = useMemo(() => {
    if (period === 'prev') return null;
    const rel = MOCK_ROWS.filter(r => release === 'all' || r.release === release);
    const prev = rel.filter(r => r.period === lastMonth);
    const cur = rel.filter(r => r.period === thisMonth);
    if (!prev.length || !cur.length) return null;
    const sum = (rows: ReportRow[], k: 'revenue' | 'plays') => rows.reduce((n, r) => n + r[k], 0);
    const prevRevenue = sum(prev, 'revenue');
    const curRevenue = sum(cur, 'revenue');
    const prevPlays = sum(prev, 'plays');
    const curPlays = sum(cur, 'plays');
    const pct = (c: number, p: number) => (p > 0 ? ((c - p) / p) * 100 : null);

    const prevByPlat = new Map<string, { revenue: number; plays: number }>();
    for (const r of prev) {
      const v = prevByPlat.get(r.platform) || { revenue: 0, plays: 0 };
      v.revenue += r.revenue; v.plays += r.plays;
      prevByPlat.set(r.platform, v);
    }
    // 인사이트는 이번 달 기준으로 계산 (byPlatform은 기간 필터 반영이라 전체 기간 선택 시 뻥튀기됨)
    const curByPlat = new Map<string, { revenue: number; plays: number }>();
    for (const r of cur) {
      const v = curByPlat.get(r.platform) || { revenue: 0, plays: 0 };
      v.revenue += r.revenue; v.plays += r.plays;
      curByPlat.set(r.platform, v);
    }
    const platChanges = [...curByPlat.entries()].map(([name, v]) => {
      const p = prevByPlat.get(name);
      return {
        name,
        revenue: v.revenue,
        prevRevenue: p?.revenue ?? 0,
        change: p ? pct(v.revenue, p.revenue) : null, // null = 신규 유입
        isNew: !p,
      };
    }).sort((a, b) => (b.revenue - b.prevRevenue) - (a.revenue - a.prevRevenue));
    const topDriver = platChanges[0];

    return {
      revenueChange: pct(curRevenue, prevRevenue),
      playsChange: pct(curPlays, prevPlays),
      prevRevenue, curRevenue, prevPlays, curPlays,
      platChanges, topDriver,
    };
  }, [period, release, lastMonth, thisMonth]);

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
    a.download = `AUDENIQ_report_${period === 'all' ? 'all' : period === 'month' ? thisMonth : lastMonth}.csv`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    setTimeout(() => URL.revokeObjectURL(url), 1000);
    toast('리포트를 CSV로 내보냈어요.');
  };

  return (
    <div id="view-reports" className="view">
      <div className="view-title">
        <div>
          <p className="eyebrow">INSIGHTS</p>
          <h1 id="reportsTitle">음악 리포트</h1>
          <p>플랫폼별 실적을 기간과 곡별로 확인해 보세요.</p>
        </div>
        <button type="button" className="button secondary" onClick={exportCsv}>CSV 내보내기 ↗</button>
      </div>

      <div className="report-tools">
        <select id="reportPeriod" aria-label="리포트 기간" value={period} onChange={e => setPeriod(e.target.value)}>
          {PERIODS.map(p => <option key={p.value} value={p.value}>{p.label}</option>)}
        </select>
        <select id="reportRelease" aria-label="발매별 필터" value={release} onChange={e => setRelease(e.target.value)}>
          <option value="all">전체 발매</option>
          {releases.map(r => <option key={r} value={r}>{r}</option>)}
        </select>
      </div>

      {/* 보고서 문서 */}
      <article className="aq-report-doc" aria-label={`${periodLabel} 음악 리포트`}>
        <header className="aq-report-doc-head">
          <p className="eyebrow">MONTHLY REPORT</p>
          <h2>{periodLabel} 음악 리포트</h2>
          <p className="muted small">발행일 {todayStr()} · AUDENIQ STUDIO</p>
        </header>

        <div className="aq-report-doc-stats">
          <div><small>집계 수익</small><strong><CountUp value={totalRevenue} format={money} /></strong></div>
          <div><small>재생 수</small><strong><CountUp value={totalPlays} format={n => `${num(n)}회`} /></strong></div>
          <div><small>플랫폼</small><strong>{platforms}곳</strong></div>
        </div>

        {insight && (
          <section aria-label="전월 대비 인사이트">
            <h3>전월 대비</h3>
            <div className="aq-insight-cards">
              <div className="aq-insight-card">
                <small>수익 변화</small>
                <strong className={insight.revenueChange != null && insight.revenueChange >= 0 ? 'up' : 'down'}>
                  {insight.revenueChange == null ? '—' : `${insight.revenueChange >= 0 ? '+' : ''}${insight.revenueChange.toFixed(1)}%`}
                </strong>
                <span>{money(insight.prevRevenue)} → {money(insight.curRevenue)}</span>
              </div>
              <div className="aq-insight-card">
                <small>재생 변화</small>
                <strong className={insight.playsChange != null && insight.playsChange >= 0 ? 'up' : 'down'}>
                  {insight.playsChange == null ? '—' : `${insight.playsChange >= 0 ? '+' : ''}${insight.playsChange.toFixed(1)}%`}
                </strong>
                <span>{num(insight.prevPlays)}회 → {num(insight.curPlays)}회</span>
              </div>
            </div>
            <ul className="aq-insight-list">
              {insight.platChanges.map(p => (
                <li key={p.name}>
                  <span>{p.name}</span>
                  {p.isNew ? (
                    <em className="aq-new-badge">신규 유입</em>
                  ) : (
                    <strong className={p.change != null && p.change >= 0 ? 'up' : 'down'}>
                      {p.change == null ? '—' : `${p.change >= 0 ? '+' : ''}${p.change.toFixed(1)}%`}
                    </strong>
                  )}
                </li>
              ))}
            </ul>
            {insight.topDriver && (
              <p className="aq-insight-note">
                이번 달 성장을 이끈 곳은 <b>{insight.topDriver.name}</b>이에요.
                {insight.platChanges.some(p => p.isNew) && (
                  <> {insight.platChanges.filter(p => p.isNew).map(p => p.name).join(', ')}에서 신규 유입도 있었어요.</>
                )}
              </p>
            )}
          </section>
        )}

        {byPlatform.length > 0 && (
          <section aria-label="플랫폼별 수익">
            <h3>플랫폼별 수익</h3>
            <ul className="aq-platform-bars">
              {byPlatform.map(([name, v]) => (
                <li key={name}>
                  <div className="aq-platform-row">
                    <span>{name}</span>
                    <strong>{money(v.revenue)}</strong>
                  </div>
                  <div className="aq-platform-track">
                    <span
                      key={`${period}-${release}`}
                      style={{ width: `${Math.max(3, Math.round((v.revenue / maxPlatformRevenue) * 100))}%` }}
                    />
                  </div>
                  <small>{num(v.plays)}회 재생</small>
                </li>
              ))}
            </ul>
          </section>
        )}

        <section aria-label="기간별 추이">
          <h3>기간별 추이</h3>
          <div id="reportGraph"><Chart key={`${period}-${release}`} rows={filtered} /></div>
        </section>

        <section aria-label="상세 내역">
          <h3>상세 내역</h3>
          {filtered.length ? (
            <ul className="aq-report-details">
              {filtered.map((r, i) => (
                <li key={i}>
                  <div className="aq-detail-top">
                    <strong>{r.platform}</strong>
                    <span>{money(r.revenue)}</span>
                  </div>
                  <div className="aq-detail-sub">
                    {r.release || '—'} / {r.track || '전체'} · {r.period} · {num(r.plays)}회 재생
                  </div>
                </li>
              ))}
            </ul>
          ) : (
            <div className="empty-page">
              <h2>아직 집계된 실적이 없어요.</h2>
              <p>플랫폼 정산이 반영되면 재생·수익 내역이 여기에 표시돼요.</p>
            </div>
          )}
        </section>
      </article>
    </div>
  );
}
