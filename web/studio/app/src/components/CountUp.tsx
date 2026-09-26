// 숫자가 부드럽게 올라가는 카운터 (요약 금액·재생 수 강조용). 모션 줄이기 설정을 존중한다.
// 프레임마다 React 상태를 바꾸면 매 프레임 리렌더가 일어나므로 DOM 텍스트를 직접 갱신한다.
import { useLayoutEffect, useRef } from 'react';

export function CountUp({ value, format = String, duration = 900 }: {
  value: number;
  format?: (n: number) => string;
  duration?: number;
}) {
  const ref = useRef<HTMLSpanElement>(null);
  const shown = useRef(0);
  const formatRef = useRef(format);
  formatRef.current = format;

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const reduce = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;
    const start = shown.current;
    if (reduce || !Number.isFinite(value) || start === value) {
      shown.current = value;
      el.textContent = formatRef.current(value);
      return;
    }
    const t0 = performance.now();
    let raf = 0;
    const tick = (now: number) => {
      const p = Math.min(1, (now - t0) / duration);
      const v = Math.round(start + (value - start) * (1 - Math.pow(1 - p, 3)));
      shown.current = v;
      el.textContent = formatRef.current(v);
      if (p < 1) raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [value, duration]);

  // 첫 렌더에는 최종값을 넣어 두어 스크린리더·JS 지연 시에도 올바른 값이 보이게 한다
  return <span ref={ref} className="aq-countup" aria-label={format(value)}>{format(value)}</span>;
}
