// 필터 칩 묶음 — 목록을 상태별로 거른다. 고른 칩은 파란 면, 개수는 옅게.
import type { ReactNode } from 'react';
import { cx } from './tone';

export interface ChipOption<T extends string> { value: T; label: ReactNode; count?: number; hidden?: boolean }

export function ChipGroup<T extends string>({ value, options, onChange, label, size, wrap, danger, hideZero = true, className }: {
  value: T;
  options: readonly ChipOption<T>[];
  onChange: (v: T) => void;
  label: string;
  size?: 'sm' | 'md';
  wrap?: boolean;
  /** 고른 칩을 빨간 면으로 (거절 사유 고르기) */
  danger?: boolean;
  /** 개수가 0이면 숫자를 숨긴다 */
  hideZero?: boolean;
  className?: string;
}) {
  return (
    <div className={cx('ui-chips', wrap && 'is-wrap', className)} role="group" aria-label={label}>
      {options.filter(o => !o.hidden).map(o => (
        <button
          key={o.value} type="button" aria-pressed={o.value === value}
          className={cx('ui-chip', size === 'sm' && 'is-sm', danger && 'is-danger')}
          onClick={() => onChange(o.value)}
        >
          {o.label}
          {o.count != null && (!hideZero || o.count > 0) && <span className="ui-chip-count">{o.count}</span>}
        </button>
      ))}
    </div>
  );
}
