// 날마다 방문 — 소개 페이지 + 스마트링크 방문을 한 막대로 (한 계열이라 범례 없이 제목이 이름).
// 막대에 손가락·마우스를 올리면 그날의 페이지·스마트링크 방문과 플랫폼 이동 수. 화면 읽기 프로그램에는 같은 숫자를 표로.
import { useEffect, useRef, useState } from 'react';
import type { DayPoint } from '../lib/artistStudio';
import { num } from '../lib/format';

const H = 168;
const PAD_TOP = 8;
const PAD_BOTTOM = 22;
const AXIS_W = 30;

const md = (day: string) => `${Number(day.slice(5, 7))}.${Number(day.slice(8, 10))}`;
const visits = (d: DayPoint) => d.page_views + d.link_views;

/** 눈금 — 0과 보기 좋은 두 칸 (1·2·5 단위) */
function ticks(max: number): number[] {
  if (max <= 0) return [0];
  const raw = max / 2;
  const mag = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 5, 10].map(m => m * mag).find(s => s >= raw) ?? raw;
  return [0, step, step * 2];
}

/** 막대 — 위 두 모서리만 4px 둥글게, 바닥에 붙인다 */
function bar(x: number, y: number, w: number, h: number): string {
  if (h <= 0) return '';
  const r = Math.min(4, w / 2, h);
  return `M${x},${y + h}V${y + r}Q${x},${y} ${x + r},${y}H${x + w - r}Q${x + w},${y} ${x + w},${y + r}V${y + h}Z`;
}

export function VisitsChart({ series }: { series: DayPoint[] }) {
  const wrap = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(320);
  const [hover, setHover] = useState<number | null>(null);
  useEffect(() => {
    const el = wrap.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setWidth(el.clientWidth));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const max = Math.max(0, ...series.map(visits));
  const scale = ticks(max);
  const top = scale[scale.length - 1] || 1;
  const plotW = Math.max(40, width - AXIS_W);
  const plotH = H - PAD_TOP - PAD_BOTTOM;
  const slot = plotW / Math.max(1, series.length);
  // 막대 사이 2px 틈, 막대는 너무 굵지 않게
  const barW = Math.max(2, Math.min(18, slot - 2));
  const y = (v: number) => PAD_TOP + plotH - (v / top) * plotH;
  const labels = series.length > 1 ? [0, Math.floor((series.length - 1) / 2), series.length - 1] : [0];
  const tip = hover != null ? series[hover] : null;
  const tipX = hover != null ? AXIS_W + slot * hover + slot / 2 : 0;

  return (
    <div className="aq-visits" ref={wrap}>
      <svg width={width} height={H} role="img" aria-label={`날마다 방문 — 가장 많은 날 ${num(max)}회`}
        onPointerLeave={() => setHover(null)}>
        {scale.map(t => (
          <g key={t} className="aq-visits-grid">
            <line x1={AXIS_W} x2={width} y1={y(t)} y2={y(t)} />
            <text x={AXIS_W - 6} y={y(t)} dy="0.32em" textAnchor="end">{num(t)}</text>
          </g>
        ))}
        {series.map((d, i) => {
          const v = visits(d);
          const x = AXIS_W + slot * i + (slot - barW) / 2;
          return (
            <g key={d.day}>
              <path className={`aq-visits-bar${hover === i ? ' is-hover' : ''}`} d={bar(x, y(v), barW, y(0) - y(v))} />
              {/* 막대보다 넓은 자리 — 가는 막대도 쉽게 짚는다 */}
              <rect x={AXIS_W + slot * i} y={PAD_TOP} width={slot} height={plotH} fill="transparent"
                onPointerEnter={() => setHover(i)} onPointerDown={() => setHover(i)} />
            </g>
          );
        })}
        {labels.map(i => (
          <text key={i} className="aq-visits-x" x={AXIS_W + slot * i + slot / 2} y={H - 6}
            textAnchor={i === 0 ? 'start' : i === series.length - 1 ? 'end' : 'middle'}>{md(series[i].day)}</text>
        ))}
      </svg>
      {tip && (
        <div className="aq-visits-tip" style={{ left: Math.min(Math.max(tipX, 80), width - 80) }} aria-hidden="true">
          <strong>{md(tip.day)} · 방문 {num(visits(tip))}회</strong>
          <span>소개 페이지 {num(tip.page_views)} · 스마트링크 {num(tip.link_views)}</span>
          <span>플랫폼으로 이동 {num(tip.clicks)}</span>
        </div>
      )}
      <table className="sr-only">
        <caption>날마다 방문</caption>
        <thead><tr><th>날짜</th><th>소개 페이지</th><th>스마트링크</th><th>플랫폼 이동</th></tr></thead>
        <tbody>{series.map(d => <tr key={d.day}><td>{d.day}</td><td>{d.page_views}</td><td>{d.link_views}</td><td>{d.clicks}</td></tr>)}</tbody>
      </table>
    </div>
  );
}
