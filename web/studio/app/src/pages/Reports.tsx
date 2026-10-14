import { useMemo, useState } from 'react';
import { useToast } from '../components/Toast';
import { CountUp } from '../ui';
import { money, num } from '../lib/format';
import { todayStr } from '../lib/date';
import { useGrowOnView, type CSSVarStyle } from '../hooks/useAnimations';
import { periodLabel as monthLabel, periods, type ReportRow } from '../data/reports';
import { useReportRows } from '../hooks/useReportRows';
import { Glyph } from '../components/Glyph';
import { Money } from '../components/Money';
import { MOCK } from '../lib/mode';
import { downloadReportsCsv, type ReportRange } from '../api/portal';
import { errorMessage } from '../api/errors';
import { MonthField } from '../components/Picker';

// 서버에서 받아 올 기간 (월 단위, 한국 시간). 그 안에서 아래 PERIODS로 다시 좁힌다.
type RangeKey = 'last12' | 'thisYear' | 'lastYear' | 'custom';
const RANGES: { value: RangeKey; label: string }[] = [
  { value: 'last12', label: '최근 12개월' },
  { value: 'thisYear', label: '올해' },
  { value: 'lastYear', label: '작년' },
  { value: 'custom', label: '직접 선택' },
];
const kstMonth = (offsetMonths = 0) => {
  const d = new Date(Date.now() + 9 * 3600_000);
  const m = d.getUTCFullYear() * 12 + d.getUTCMonth() + offsetMonths;
  return `${Math.floor(m / 12)}-${String((m % 12) + 1).padStart(2, '0')}`;
};
function rangeFor(key: RangeKey, from: string, to: string): ReportRange {
  const year = kstMonth().slice(0, 4);
  if (key === 'thisYear') return { from: `${year}-01`, to: kstMonth() };
  if (key === 'lastYear') return { from: `${Number(year) - 1}-01`, to: `${Number(year) - 1}-12` };
  if (key === 'custom' && from && to && from <= to) return { from, to };
  return {};
}

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

const DETAIL_PREVIEW = 6;
const pctText = (v: number | null) => (v == null ? '—' : `${v >= 0 ? '+' : ''}${v.toFixed(1)}%`);
const changeTone = (v: number | null) => (v == null ? '' : v >= 0 ? 'is-up' : 'is-down');

export function Reports() {
  const toast = useToast();
  const [rangeKey, setRangeKey] = useState<RangeKey>('last12');
  const [customFrom, setCustomFrom] = useState(() => kstMonth(-11));
  const [customTo, setCustomTo] = useState(() => kstMonth());
  const range = useMemo(() => rangeFor(rangeKey, customFrom, customTo), [rangeKey, customFrom, customTo]);
  const customInvalid = rangeKey === 'custom' && (!customFrom || !customTo || customFrom > customTo);
  const [period, setPeriod] = useState('all');
  const [release, setRelease] = useState('all');
  const [showAll, setShowAll] = useState(false);

  const { rows: MOCK_ROWS, loading } = useReportRows(range); // 체험 모드는 예시 행, 실서버는 플랫폼 리포트
  const [thisMonth = '', lastMonth = ''] = periods(MOCK_ROWS);

  const filtered = useMemo(() => MOCK_ROWS.filter(r => {
    if (period === 'month' && r.period !== thisMonth) return false;
    if (period === 'prev' && r.period !== lastMonth) return false;
    if (release !== 'all' && r.release !== release) return false;
    return true;
  }), [MOCK_ROWS, period, release, thisMonth, lastMonth]);

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

  const rangeLabel = RANGES.find(r => r.value === rangeKey)?.label ?? '';
  const periodLabel = period === 'month' ? monthLabel(thisMonth) : period === 'prev' ? monthLabel(lastMonth)
    : rangeKey === 'custom' && !customInvalid ? `${customFrom} ~ ${customTo}` : rangeLabel;

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
  }, [MOCK_ROWS, period, release, lastMonth, thisMonth]);

  const [exporting, setExporting] = useState(false);
  const exportCsv = async () => {
    // 실서버: 선택한 기간 전체를 서버가 만든다 (화면 미리보기 제한 없이 모든 행). 발매·달 필터는 화면에서만 적용된다.
    if (!MOCK && period === 'all' && release === 'all') {
      setExporting(true);
      try {
        await downloadReportsCsv(range);
        toast('리포트를 CSV로 내보냈어요.');
      } catch (e) {
        toast(errorMessage(e));
      } finally {
        setExporting(false);
      }
      return;
    }
    if (!filtered.length) { toast('내보낼 리포트가 없어요.'); return; }
    const cell = (v: string | number) => {
      const t = String(v ?? '');
      const safe = typeof v === 'string' && /^[=+\-@]/.test(t) ? `'${t}` : t;
      return `"${safe.replace(/"/g, '""')}"`;
    };
    const header = 'period,platform,releaseTitle,track,plays,revenue';
    const lines = filtered.map(r =>
      [r.period, r.platform, r.release, r.track, r.plays, r.revenue].map(cell).join(','));
    const csv = '\uFEFF' + [header, ...lines].join('\r\n');
    const blob = new Blob([csv], { type: 'text/csv;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `AUDENIQ_report_${period === 'all' ? (range.from ? `${range.from}_${range.to}` : 'last12') : period === 'month' ? thisMonth : lastMonth}.csv`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    setTimeout(() => URL.revokeObjectURL(url), 1000);
    toast('리포트를 CSV로 내보냈어요.');
  };

  return (
    <div id="view-reports" className="view">
      <div className="ui-page-head">
        <div>
          <h1 id="reportsTitle">음악 리포트</h1>
        </div>
        <button type="button" className="ui-btn is-soft" onClick={exportCsv} disabled={exporting}><Glyph name="download" size={15} className="aq-glyph-lead" />{exporting ? '내보내는 중…' : 'CSV 내보내기'}</button>
      </div>

      <div className="report-tools">
        <select id="reportRange" className="ui-select" aria-label="불러올 기간" value={rangeKey} onChange={e => { setRangeKey(e.target.value as RangeKey); setPeriod('all'); }}>
          {RANGES.map(r => <option key={r.value} value={r.value}>{r.label}</option>)}
        </select>
        {rangeKey === 'custom' && (<>
          <MonthField id="reportFrom" aria-label="시작 월" value={customFrom} max={customTo || undefined} onChange={setCustomFrom} />
          <MonthField id="reportTo" aria-label="끝 월" value={customTo} min={customFrom || undefined} max={kstMonth()} onChange={setCustomTo} />
        </>)}
        <select id="reportPeriod" className="ui-select" aria-label="리포트 기간" value={period} onChange={e => setPeriod(e.target.value)}>
          {PERIODS.map(p => <option key={p.value} value={p.value}>{p.value === 'all' ? '선택 기간 전체' : p.label}</option>)}
        </select>
        <select id="reportRelease" className="ui-select" aria-label="발매별 필터" value={release} onChange={e => setRelease(e.target.value)}>
          <option value="all">전체 발매</option>
          {releases.map(r => <option key={r} value={r}>{r}</option>)}
        </select>
      </div>

      {customInvalid && <p className="ui-field-help aq-help-error" role="alert">시작 월이 끝 월보다 늦어요. 기간을 다시 골라 주세요.</p>}
      {loading && <p className="ui-field-help" aria-live="polite">리포트를 불러오고 있어요…</p>}

      <div className="aq-report">
        <section className="aq-report-hero" aria-labelledby="reportHeroHeading">
          <h2 id="reportHeroHeading">{periodLabel} 수익</h2>
          <strong className="aq-report-amount"><Money value={totalRevenue} animate /></strong>
          {insight?.revenueChange != null && (
            <span className={`aq-report-change ${insight.revenueChange >= 0 ? 'is-up' : 'is-down'}`}>
              지난달보다 {pctText(insight.revenueChange)}
            </span>
          )}
          <p className="aq-report-sub">
            재생 <CountUp value={totalPlays} format={n => `${num(n)}회`} /> · 플랫폼 {platforms}곳 · 발행일 {todayStr()}
          </p>
        </section>

        {insight && (
          <section className="aq-rp-card" aria-label="전월 대비 인사이트">
            <h3>지난달과 비교</h3>
            <ul className="aq-report-rows">
              <li>
                <div className="min-0"><strong>수익</strong><small><Money value={insight.prevRevenue} /> → <Money value={insight.curRevenue} /></small></div>
                <b className={changeTone(insight.revenueChange)}>{pctText(insight.revenueChange)}</b>
              </li>
              <li>
                <div className="min-0"><strong>재생</strong><small>{num(insight.prevPlays)}회 → {num(insight.curPlays)}회</small></div>
                <b className={changeTone(insight.playsChange)}>{pctText(insight.playsChange)}</b>
              </li>
            </ul>
            <h4>플랫폼별 변화</h4>
            <ul className="aq-report-rows is-compact">
              {insight.platChanges.map(p => (
                <li key={p.name}>
                  <strong>{p.name}</strong>
                  {p.isNew ? <em className="aq-new-badge">신규 유입</em> : <b className={changeTone(p.change)}>{pctText(p.change)}</b>}
                </li>
              ))}
            </ul>
            {insight.topDriver && (
              <p className="aq-report-note">
                이번 달 성장을 이끈 곳은 <b>{insight.topDriver.name}</b>이에요.
                {insight.platChanges.some(p => p.isNew) && (
                  <> {insight.platChanges.filter(p => p.isNew).map(p => p.name).join(', ')}에서 신규 유입도 있었어요.</>
                )}
              </p>
            )}
          </section>
        )}

        {byPlatform.length > 0 && (
          <section className="aq-rp-card" aria-label="플랫폼별 수익">
            <h3>플랫폼별 수익</h3>
            <ul className="aq-platform-bars">
              {byPlatform.map(([name, v]) => (
                <li key={name}>
                  <div className="aq-platform-row">
                    <span>{name}</span>
                    <strong><Money value={v.revenue} /></strong>
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

        <section className="aq-rp-card" aria-label="기간별 추이">
          <h3>기간별 추이</h3>
          <div id="reportGraph"><Chart key={`${period}-${release}`} rows={filtered} /></div>
        </section>

        <section className="aq-rp-card" aria-label="상세 내역">
          <h3>상세 내역 {filtered.length > 0 && <span className="aq-report-count">{filtered.length}</span>}</h3>
          {filtered.length ? (
            <>
              <ul className="aq-report-rows aq-report-details">
                {(showAll ? filtered : filtered.slice(0, DETAIL_PREVIEW)).map((r, i) => (
                  <li key={i}>
                    <div className="min-0">
                      <strong>{r.platform}</strong>
                      <small>{r.release || '—'} / {r.track || '전체'} · {r.period} · {num(r.plays)}회 재생</small>
                    </div>
                    <b><Money value={r.revenue} /></b>
                  </li>
                ))}
              </ul>
              {filtered.length > DETAIL_PREVIEW && (
                <button type="button" className="ui-btn is-white aq-report-more" aria-expanded={showAll} onClick={() => setShowAll(v => !v)}>
                  {showAll ? '접기' : `전체 ${filtered.length}건 보기`}
                  <Glyph name="chevron-down" size={15} className={showAll ? 'is-flip' : ''} />
                </button>
              )}
            </>
          ) : (
            <div className="ui-empty">
              <h2>아직 집계된 실적이 없어요.</h2>
              <p>플랫폼 정산이 반영되면 재생·수익 내역이 여기에 표시돼요.</p>
            </div>
          )}
        </section>
      </div>
    </div>
  );
}
