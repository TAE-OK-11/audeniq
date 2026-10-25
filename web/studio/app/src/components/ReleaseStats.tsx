// 발매 상세 '성과' 탭 — 이 발매의 플랫폼 리포트(최근 12개월): 재생·수익 합계, 지난달 대비, 달별 막대, 플랫폼별 몫.
// 음악 리포트와 같은 원천(useReportRows · GET /api/orgs/{org}/reports)에서 이 발매 행만 고른다. 원화 확정 전 금액은 더하지 않는다.
import { useMemo } from 'react';
import { useNavigate } from '../lib/router';
import { useReportRows } from '../hooks/useReportRows';
import { periodLabel } from '../data/reports';
import { num } from '../lib/format';
import { platformKey, platformLabel } from '../lib/platformNames';
import { CountUp } from '../ui';
import { BrandMark } from './BrandMark';
import { Glyph } from './Glyph';
import { Money } from './Money';
import { SkeletonRows } from './Skeleton';

export function ReleaseStats({ releaseId, title }: { releaseId: string; title: string }) {
  const nav = useNavigate();
  const { rows, pending, error } = useReportRows();
  const mine = useMemo(() => rows.filter(r => (r.releaseId ? r.releaseId === releaseId : r.release === title)), [rows, releaseId, title]);
  const s = useMemo(() => {
    const months = new Map<string, { plays: number; revenue: number }>();
    const dsps = new Map<string, { plays: number; revenue: number }>();
    for (const r of mine) {
      const m = months.get(r.period) ?? { plays: 0, revenue: 0 };
      m.plays += r.plays; m.revenue += r.revenue; months.set(r.period, m);
      const d = dsps.get(r.platform) ?? { plays: 0, revenue: 0 };
      d.plays += r.plays; d.revenue += r.revenue; dsps.set(r.platform, d);
    }
    const byMonth = [...months.entries()].sort(([a], [b]) => a.localeCompare(b)).slice(-12);
    const byDsp = [...dsps.entries()].sort((a, b) => b[1].plays - a[1].plays);
    const plays = byDsp.reduce((n, [, v]) => n + v.plays, 0);
    const revenue = byDsp.reduce((n, [, v]) => n + v.revenue, 0);
    const last = byMonth.at(-1)?.[1].plays ?? 0;
    const prev = byMonth.at(-2)?.[1].plays ?? 0;
    const change = prev > 0 ? Math.round(((last - prev) / prev) * 100) : null;
    return { byMonth, byDsp, plays, revenue, change, lastMonth: byMonth.at(-1)?.[0] ?? '', top: Math.max(1, ...byMonth.map(([, v]) => v.plays)) };
  }, [mine]);

  if (pending) return <SkeletonRows count={3} />;
  if (error) return <div className="ui-empty"><h2>리포트를 불러오지 못했어요.</h2><p>{error}</p></div>;
  if (!mine.length) {
    return (
      <section className="aq-relinfo-card aq-stats-empty">
        <span className="ui-icon is-lg" aria-hidden="true"><Glyph name="chart" size={24} /></span>
        <h2 className="subhead">아직 이 발매의 리포트가 없어요</h2>
        <p>플랫폼 정산 리포트는 보통 발매한 달이 끝나고 1~2달 뒤에 들어와요. 들어오면 여기서 재생과 수익을 볼 수 있어요.</p>
      </section>
    );
  }
  return (
    <div className="aq-detail-stack">
      <section className="aq-relinfo-card aq-stats-hero" aria-labelledby="statsTitle">
        <h2 id="statsTitle" className="aq-stats-label">최근 12개월 재생</h2>
        <p className="aq-stats-big"><CountUp value={s.plays} format={num} /><small>회</small></p>
        <div className="aq-stats-sub">
          <span>수익 <b><Money value={s.revenue} /></b></span>
          {s.change !== null && (
            <span className={`aq-stats-change${s.change >= 0 ? ' is-up' : ' is-down'}`}>
              <Glyph name="arrow-up" size={12} />{Number(s.lastMonth.slice(5))}월 재생이 지난달보다 {Math.abs(s.change)}% {s.change >= 0 ? '늘었어요' : '줄었어요'}
            </span>
          )}
        </div>
        {s.byMonth.length > 1 && (
          <ol className="aq-stats-bars" aria-label="달별 재생">
            {s.byMonth.map(([m, v], i) => (
              <li key={m} className={i === s.byMonth.length - 1 ? 'is-last' : undefined} title={`${periodLabel(m)} ${num(v.plays)}회`}>
                <i style={{ height: `${Math.max(4, Math.round((v.plays / s.top) * 100))}%` }} />
                <span>{Number(m.slice(5))}월</span>
              </li>
            ))}
          </ol>
        )}
      </section>
      <section className="aq-relinfo-card" aria-labelledby="statsDspTitle">
        <div className="aq-tracklist-head">
          <h2 id="statsDspTitle" className="subhead">플랫폼별</h2>
          <button type="button" className="aq-head-link" onClick={() => nav('/reports')}>음악 리포트<Glyph name="chevron-right" size={14} /></button>
        </div>
        <ul className="aq-stats-dsps">
          {s.byDsp.map(([name, v]) => {
            const k = platformKey(name);
            const share = s.plays ? Math.round((v.plays / s.plays) * 100) : 0;
            return (
              <li key={name}>
                {k ? <BrandMark id={k} label={platformLabel(name)} size={32} /> : <span className="ui-icon is-sm is-gray" aria-hidden="true"><Glyph name="music" size={16} /></span>}
                <span className="aq-stats-dsp-main">
                  <span className="aq-stats-dsp-top"><b>{platformLabel(name)}</b><span>{num(v.plays)}회 · {share}%</span></span>
                  <span className="aq-stats-share" aria-hidden="true"><i style={{ width: `${share}%` }} /></span>
                </span>
                <span className="aq-stats-dsp-rev"><Money value={v.revenue} /></span>
              </li>
            );
          })}
        </ul>
      </section>
    </div>
  );
}
