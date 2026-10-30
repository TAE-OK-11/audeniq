// 최근 몇 달 막대 — 이번 달만 진하게. 홈 '음악 리포트'(재생 수)와 퍼블리싱 '분석'(저작권료)이 같이 쓴다
import { periodLabel } from '../data/reports';
import type { CSSVarStyle } from '../hooks/useAnimations';
import { useGrowOnView } from '../hooks/useAnimations';
import '../styles/split/Dashboard.css';

export function MiniTrend({ data, label, short, format }: {
  data: { period: string; value: number }[];
  label: string;
  /** 이번 달 막대 위 짧은 숫자 */
  short: (n: number) => string;
  /** 막대에 손을 올렸을 때 보이는 숫자 */
  format: (n: number) => string;
}) {
  const ref = useGrowOnView<HTMLDivElement>('.aq-mini-bar');
  const high = Math.max(1, ...data.map(d => d.value));
  return (
    <div className="aq-mini-trend" ref={ref} role="img" aria-label={label}>
      {data.map((d, i) => (
        <span key={d.period} className="aq-mini-col">
          {/* 막대는 모두 같은 그림 칸 안에서 같은 비율로 — 숫자 줄이 한 칸에만 있어도 높이가 틀어지지 않게 */}
          <span className="aq-mini-plot">
            <span
              className={`aq-mini-bar${i === data.length - 1 ? ' is-current' : ''}`}
              style={{ '--h': `${Math.max(6, Math.round((d.value / high) * 100))}%`, '--d': `${i * 80}ms` } as CSSVarStyle}
              title={`${periodLabel(d.period)} · ${format(d.value)}`}
            >
              {/* 숫자는 이번 달 막대에만 — 막대마다 붙이면 아무것도 눈에 안 띈다. 나머지는 막대에 손을 올리면 보인다 */}
              {i === data.length - 1 && <b className="aq-mini-val">{short(d.value)}</b>}
            </span>
          </span>
          <small>{Number(d.period.slice(5))}월</small>
        </span>
      ))}
    </div>
  );
}
