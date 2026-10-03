// AUDENIQ 고르기 토글 — 옅은 바탕 위 흰 알약이 고른 칸으로 미끄러진다 (서류 준비 방법, 정산 내역 구분 …)
import type { ReactNode } from 'react';
import { useSegPill } from '../hooks/useSegPill';

export function Segmented<T extends string>({ value, options, onChange, label, labelledBy, className = '', tabs = false }: {
  value: T;
  options: readonly { value: T; label: ReactNode }[];
  onChange: (v: T) => void;
  label?: string;
  labelledBy?: string;
  className?: string;
  /** 아래 내용을 바꾸는 탭이면 tablist/tab, 값을 고르는 거면 radiogroup/radio */
  tabs?: boolean;
}) {
  const { ref, live, pillStyle } = useSegPill(value);
  return (
    <div
      ref={ref}
      className={`aq-seg${live ? ' is-live' : ''}${className ? ` ${className}` : ''}`}
      role={tabs ? 'tablist' : 'radiogroup'} aria-label={label} aria-labelledby={labelledBy}
      style={{ gridTemplateColumns: `repeat(${options.length},minmax(0,1fr))` }}
    >
      <span className="aq-seg-pill" aria-hidden="true" style={pillStyle} />
      {options.map(o => (
        <button
          key={o.value} type="button"
          role={tabs ? 'tab' : 'radio'}
          {...(tabs ? { 'aria-selected': o.value === value } : { 'aria-checked': o.value === value })}
          onClick={() => onChange(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}
