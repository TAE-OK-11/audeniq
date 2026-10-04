// 업무 화면 차트 — 외부 라이브러리 없이 SVG·CSS로. 얇은 막대, 끝 4px 둥글게, 막대 사이 2px 틈,
// 값과 이름은 글자색(ink)으로, 색은 브랜드 파랑 하나(+ 두 번째 계열은 옅은 파랑). 마우스를 올리면 값이 보인다.
import { useId, useState, type ReactNode } from 'react';

/** 추세선 — 한 계열, 마지막 점만 강조 */
export function Sparkline({ values, label, unit = '', width = 120, height = 34, bad }: { values: number[]; label: string; unit?: string; width?: number; height?: number; bad?: boolean }) {
  if (values.length < 2) return null;
  const max = Math.max(...values, 0.0001);
  const step = (width - 8) / (values.length - 1);
  const pts = values.map((v, i) => [4 + i * step, height - 5 - (v / max) * (height - 10)] as const);
  const d = pts.map(([x, y], i) => `${i ? 'L' : 'M'}${x.toFixed(1)},${y.toFixed(1)}`).join('');
  const last = pts[pts.length - 1];
  return (
    <svg className={`adm-w-spark${bad ? ' is-bad' : ''}`} viewBox={`0 0 ${width} ${height}`} width={width} height={height} role="img" aria-label={`${label}: ${values.map(v => `${v}${unit}`).join(', ')}`}>
      <path d={d} fill="none" strokeWidth="2" strokeLinejoin="round" strokeLinecap="round" />
      {pts.map(([x, y], i) => (
        <g key={i}>
          <circle cx={x} cy={y} r="8" fill="transparent"><title>{`${values.length - 1 - i === 0 ? '이번 주' : `${values.length - 1 - i}주 전`} ${values[i]}${unit}`}</title></circle>
        </g>
      ))}
      <circle cx={last[0]} cy={last[1]} r="4" className="adm-w-spark-dot" />
    </svg>
  );
}

/** 가로 막대 목록 — 이름 · 막대 · 값 (반려 사유, 유형별 건수, DSP 실패율) */
export function BarList({ items, format = v => String(v), max }: { items: { key: string; label: ReactNode; value: number; hint?: string; bad?: boolean }[]; format?: (v: number) => string; max?: number }) {
  const top = max ?? Math.max(...items.map(i => i.value), 0.0001);
  return (
    <ul className="adm-w-bars">
      {items.map(i => (
        <li key={i.key} title={i.hint ? `${i.hint}` : undefined}>
          <span className="adm-w-bars-label">{i.label}</span>
          <span className="adm-w-bars-track" aria-hidden="true"><span className={i.bad ? 'is-bad' : undefined} style={{ width: `${Math.max(2, (i.value / top) * 100)}%` }} /></span>
          <b>{format(i.value)}</b>
        </li>
      ))}
    </ul>
  );
}

/** 하루 처리량 — 사람 처리 + 자동 처리 쌓은 막대. 범례·마우스 툴팁·표 보기 */
export function DailyChart({ rows }: { rows: { date: string; manual: number; auto: number }[] }) {
  const [hover, setHover] = useState<number | null>(null);
  const [table, setTable] = useState(false);
  const id = useId();
  const H = 180;
  const max = Math.max(...rows.map(r => r.manual + r.auto), 1);
  const nice = Math.ceil(max / 20) * 20;
  const W = Math.max(rows.length * 22, 300);
  const bw = Math.max(6, W / rows.length - 4);
  const y = (v: number) => H - (v / nice) * H;
  const h = hover != null ? rows[hover] : null;
  const fmtDay = (d: string) => `${Number(d.slice(5, 7))}/${Number(d.slice(8, 10))}`;

  return (
    <div className="adm-w-chart">
      <div className="adm-w-chart-top">
        <span className="adm-w-legend" id={`${id}-legend`}>
          <span><i className="is-manual" />사람 처리</span>
          <span><i className="is-auto" />자동 처리</span>
        </span>
        <button type="button" className="adm-link-btn" onClick={() => setTable(v => !v)}>{table ? '차트로 보기' : '표로 보기'}</button>
      </div>
      {table ? (
        <div className="adm-table-wrap">
          <table className="adm-table"><thead><tr><th>날짜</th><th>사람 처리</th><th>자동 처리</th><th>자동 비율</th></tr></thead>
            <tbody>{rows.map(r => <tr key={r.date}><td>{r.date}</td><td>{r.manual}</td><td>{r.auto}</td><td>{Math.round((r.auto / Math.max(1, r.auto + r.manual)) * 100)}%</td></tr>)}</tbody>
          </table>
        </div>
      ) : (
        <div className="adm-w-chart-box" onMouseLeave={() => setHover(null)}>
          <svg viewBox={`-30 -8 ${W + 34} ${H + 30}`} preserveAspectRatio="none" role="img" aria-labelledby={`${id}-legend`} aria-label="하루 처리량 차트">
            {[0, 0.5, 1].map(f => (
              <g key={f}>
                <line x1="0" x2={W} y1={y(nice * f)} y2={y(nice * f)} className="adm-w-grid" />
                <text x="-6" y={y(nice * f) + 4} textAnchor="end" className="adm-w-axis">{nice * f}</text>
              </g>
            ))}
            {rows.map((r, i) => {
              const x = i * (W / rows.length) + 2;
              const hm = (r.manual / nice) * H;
              const ha = (r.auto / nice) * H;
              return (
                <g key={r.date} onMouseEnter={() => setHover(i)} className={hover != null && hover !== i ? 'is-dim' : undefined}>
                  <rect x={x - 2} y={0} width={bw + 4} height={H} fill="transparent" />
                  {/* 아래: 사람 처리(진한 파랑, 바닥에 붙음) / 위: 자동 처리(옅은 파랑, 위 끝 둥글게) — 사이 2px 틈 */}
                  <rect x={x} y={H - hm} width={bw} height={Math.max(0, hm)} className="adm-w-bar-manual" />
                  {ha > 0 && <path className="adm-w-bar-auto" d={roundTop(x, H - hm - 2 - ha, bw, ha, 4)} />}
                  {(i % Math.ceil(rows.length / 8) === 0 || i === rows.length - 1) && <text x={x + bw / 2} y={H + 18} textAnchor="middle" className="adm-w-axis">{fmtDay(r.date)}</text>}
                </g>
              );
            })}
          </svg>
          {h && hover != null && (
            <div className="adm-w-tip" style={{ left: `${((hover + 0.5) / rows.length) * 100}%` }} role="status">
              <b>{h.date}</b>
              <span><i className="is-manual" />사람 {h.manual}건</span>
              <span><i className="is-auto" />자동 {h.auto}건</span>
              <small>자동 비율 {Math.round((h.auto / Math.max(1, h.auto + h.manual)) * 100)}%</small>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function roundTop(x: number, y: number, w: number, h: number, r: number) {
  const rr = Math.min(r, w / 2, h);
  return `M${x},${y + h}V${y + rr}Q${x},${y} ${x + rr},${y}H${x + w - rr}Q${x + w},${y} ${x + w},${y + rr}V${y + h}Z`;
}
