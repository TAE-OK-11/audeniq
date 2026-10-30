import { useEffect, useMemo, useState } from 'react';
import { useToast } from '../components/Toast';
import { CountUp } from '../ui';
import { Segmented } from '../components/Segmented';
import { money, num } from '../lib/format';
import { todayStr } from '../lib/date';
import { useGrowOnView, type CSSVarStyle } from '../hooks/useAnimations';
import { periodLabel as monthLabel, periods, prevMonth, type ReportRow } from '../data/reports';
import { useReportRows } from '../hooks/useReportRows';
import { Glyph } from '../components/Glyph';
import { Money } from '../components/Money';
import { MOCK } from '../lib/mode';
import { downloadReportsCsv, type ReportRange } from '../api/portal';
import { errorMessage } from '../api/errors';
import { MonthField } from '../components/Picker';
import { platformKey, platformLabel } from '../lib/platformNames';
import { BrandMark } from '../components/BrandMark';
import '../styles/split/Reports.css';

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
  { value: 'all', label: '모든 달 합계' },
  { value: 'month', label: '최근 달' },
  { value: 'prev', label: '그 전 달' },
] as const;

/** '2026-09' → '9월' (해가 바뀌는 1월은 연도까지) */
const monthShort = (k: string) => (/^\d{4}-\d{2}$/.test(k) ? (k.endsWith('-01') ? `${k.slice(2, 4)}년 1월` : `${+k.slice(5)}월`) : k);
/** 막대 위 짧은 금액 — 1만 원 이상은 '1.2만', 그 아래는 그대로 */
const shortWon = (v: number) => (v >= 10000 ? `${(v / 10000).toFixed(v >= 100000 ? 0 : 1).replace(/\.0$/, '')}만` : num(Math.round(v)));

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
      {/* 막대마다 금액, 아래에는 모든 달 — 예전엔 값 없이 처음·끝 달만 있어 몇 달치인지·얼마인지 알 수 없었다 */}
      <div className="report-chart has-values" ref={chartRef} role="img" aria-label={`기간별 수익 막대그래프: ${vals.map(([k, v]) => `${monthShort(k)} ${money(v)}`).join(', ')}`}>
        {vals.map(([k, v]) => (
          <div key={k} className="report-col">
            <span className="report-val" aria-hidden="true">{shortWon(v)}</span>
            <div className="report-bar" style={barStyle(v)} title={`${k} · ${money(v)}`} />
          </div>
        ))}
      </div>
      <div className="report-axis is-all" aria-hidden="true">
        {vals.map(([k]) => <span key={k}>{monthShort(k)}</span>)}
      </div>
    </>
  );
}

const DETAIL_PREVIEW = 6;
const pctText = (v: number | null) => (v == null ? '—' : `${v >= 0 ? '+' : ''}${v.toFixed(1)}%`);
const changeTone = (v: number | null) => (v == null ? '' : v >= 0 ? 'is-up' : 'is-down');


/** 플랫폼 로고 — 아는 곳만 (모르는 곳은 이름만) */
function PlatformMark({ name }: { name: string }) {
  const k = platformKey(name);
  return k ? <BrandMark id={k} label={platformLabel(name)} size={22} /> : null;
}

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

  const { rows: MOCK_ROWS, loading, pending } = useReportRows(range); // 체험 모드는 예시 행, 실서버는 플랫폼 리포트
  // 지난달은 달력상 바로 앞 달 — 데이터가 있는 달 두 개를 고르면 빈 달을 건너뛰어 두 달 전과 '지난달'로 비교했다
  const [thisMonth = ''] = periods(MOCK_ROWS);
  const lastMonth = prevMonth(thisMonth);
  // 고른 발매가 바뀐 기간에 없으면 '전체 발매'로 — 목록에는 '전체 발매'가 보이는데 숨은 필터로 0원이 나왔다
  useEffect(() => {
    if (release !== 'all' && !loading && !MOCK_ROWS.some(r => r.release === release)) setRelease('all');
  }, [MOCK_ROWS, release, loading]);

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
          <p className="eyebrow">수익·성과</p>
          <h1 id="reportsTitle">음악 리포트</h1>
          <p>플랫폼별 재생과 수익을 기간·곡마다 나눠 봐요.</p>
        </div>
      </div>

      <div className="report-tools">
        <select id="reportRange" className="ui-select" aria-label="불러올 기간" title="불러올 기간" value={rangeKey} onChange={e => { setRangeKey(e.target.value as RangeKey); setPeriod('all'); }}>
          {RANGES.map(r => <option key={r.value} value={r.value}>{r.label}</option>)}
        </select>
        {rangeKey === 'custom' && (<>
          <MonthField id="reportFrom" aria-label="시작 월" value={customFrom} max={customTo || undefined} onChange={setCustomFrom} />
          <MonthField id="reportTo" aria-label="끝 월" value={customTo} min={customFrom || undefined} max={kstMonth()} onChange={setCustomTo} />
        </>)}
        <select id="reportRelease" className="ui-select" aria-label="발매별 필터" value={release} onChange={e => setRelease(e.target.value)}>
          <option value="all">전체 발매</option>
          {releases.map(r => <option key={r} value={r}>{r}</option>)}
        </select>
      </div>

      {customInvalid && <p className="ui-field-help aq-help-error" role="alert">시작 월이 끝 월보다 늦어요. 기간을 다시 골라 주세요.</p>}
      {/* 불러오는 중 안내는 화면 읽기 프로그램에만 — 보이는 한 줄이 생겼다 사라지며 아래 카드를 밀어 올리고 내렸다 */}
      <p className="sr-only" aria-live="polite">{loading ? '리포트를 불러오고 있어요.' : ''}</p>

      {pending ? <ReportSkeleton title={`${periodLabel} 수익`} /> : (
      <div className="aq-report">
        <section className="aq-report-hero" aria-labelledby="reportHeroHeading">
          {/* 고른 기간 안에서 어느 달을 볼지는 숫자 바로 위에서 — 다른 화면과 같은 세그먼트(회색 바탕 + 흰 알약)로.
              각진 칩은 AUDENIQ의 다른 전환 버튼과 모양이 달랐다 */}
          <Segmented className="aq-report-period" label="리포트 기간" value={period as typeof PERIODS[number]['value']} options={PERIODS} onChange={setPeriod} />
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
                  <span className="aq-report-plat"><PlatformMark name={p.name} /><strong>{platformLabel(p.name)}</strong></span>
                  {p.isNew ? <em className="aq-new-badge">신규 유입</em> : <b className={changeTone(p.change)}>{pctText(p.change)}</b>}
                </li>
              ))}
            </ul>
            {insight.topDriver && (
              <p className="aq-report-note">
                이번 달 성장을 이끈 곳은 <b>{platformLabel(insight.topDriver.name)}</b>이에요.
                {insight.platChanges.some(p => p.isNew) && (
                  <> {insight.platChanges.filter(p => p.isNew).map(p => platformLabel(p.name)).join(', ')}에서 신규 유입도 있었어요.</>
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
                    <span className="aq-report-plat"><PlatformMark name={name} /><span>{platformLabel(name)}</span></span>
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
          {/* 내보내기는 상세 내역을 받는 일이라 그 카드에 — 화면 맨 위 첫 버튼일 만큼 자주 쓰지 않는다 */}
          <div className="aq-rp-head">
            <h3>상세 내역 {filtered.length > 0 && <span className="aq-report-count">{filtered.length}</span>}</h3>
            {filtered.length > 0 && (
              <button type="button" className="ui-btn is-soft is-sm" onClick={exportCsv} disabled={exporting}><Glyph name="download" size={14} className="aq-glyph-lead" />{exporting ? '내보내는 중…' : 'CSV 내보내기'}</button>
            )}
          </div>
          {filtered.length ? (
            <>
              <ul className="aq-report-rows aq-report-details">
                {(showAll ? filtered : filtered.slice(0, DETAIL_PREVIEW)).map((r, i) => (
                  <li key={i}>
                    <div className="min-0">
                      <strong>{platformLabel(r.platform)}</strong>
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
      )}
    </div>
  );
}

/** 처음 불러오는 동안 — 실제 카드와 같은 칸·제목·대략 같은 높이로 자리를 잡아 둔다.
 *  데이터가 오면 이 자리가 그대로 채워질 뿐이라 아래 카드가 400px씩 밀려 내려가지 않는다 */
function ReportSkeleton({ title }: { title: string }) {
  const rows = (n: number, compact = false) => (
    <ul className={`aq-report-rows${compact ? ' is-compact' : ''}`}>
      {Array.from({ length: n }, (_, i) => (
        <li key={i}>
          <div className="min-0">
            <span className="aq-skel aq-skel-line" style={{ width: `${5 + (i % 3)}em` }} />
            {!compact && <span className="aq-skel aq-skel-line is-sub" style={{ width: `${9 + (i % 2) * 3}em` }} />}
          </div>
          <span className="aq-skel aq-skel-line" style={{ width: '4.5em' }} />
        </li>
      ))}
    </ul>
  );
  return (
    <div className="aq-report is-pending" aria-busy="true">
      <section className="aq-report-hero" aria-labelledby="reportHeroHeadingPending">
        <h2 id="reportHeroHeadingPending">{title}</h2>
        <span className="aq-skel aq-report-skel-amount" />
        <span className="aq-skel aq-report-skel-pill" />
        <span className="aq-skel aq-skel-line is-sub aq-report-skel-sub" />
      </section>
      <section className="aq-rp-card" aria-label="전월 대비 인사이트">
        <h3>지난달과 비교</h3>
        {rows(2)}
        <h4>플랫폼별 변화</h4>
        {rows(4, true)}
        <span className="aq-skel aq-report-skel-note" />
      </section>
      <section className="aq-rp-card" aria-label="플랫폼별 수익">
        <h3>플랫폼별 수익</h3>
        <ul className="aq-platform-bars">
          {Array.from({ length: 4 }, (_, i) => (
            <li key={i}>
              <div className="aq-platform-row">
                <span className="aq-skel aq-skel-line" style={{ width: `${5 + i}em` }} />
                <span className="aq-skel aq-skel-line" style={{ width: '4.5em' }} />
              </div>
              <div className="aq-platform-track" />
              <span className="aq-skel aq-skel-line is-sub" style={{ width: '5em' }} />
            </li>
          ))}
        </ul>
      </section>
      <section className="aq-rp-card" aria-label="기간별 추이">
        <h3>기간별 추이</h3>
        <div id="reportGraphPending"><span className="aq-skel aq-report-skel-chart" /></div>
      </section>
      <section className="aq-rp-card" aria-label="상세 내역">
        <h3>상세 내역</h3>
        {rows(5)}
      </section>
    </div>
  );
}
