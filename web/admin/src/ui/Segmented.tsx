// 고르기 토글 — 몇 가지 보기 중 하나. 옅은 회색 바닥 위 흰 알약이 고른 칸으로 미끄러진다.
// (목록 거르기는 ChipGroup, 화면 안 하위 화면 전환은 Tabs)
import type { ReactNode } from 'react';
import { useSegPill } from '../hooks/useSegPill';
import { cx } from './tone';

export interface SegOption<T extends string> { value: T; label: ReactNode; disabled?: boolean }

export function Segmented<T extends string>({ value, options, onChange, label, labelledBy, size, fill, tabs = false, className }: {
  value: T;
  options: readonly SegOption<T>[];
  onChange: (v: T) => void;
  label?: string;
  labelledBy?: string;
  size?: 'sm' | 'md';
  /** 칸을 같은 폭으로 나눠 줄을 가득 채운다 */
  fill?: boolean;
  /** 아래 내용을 바꾸는 탭이면 tablist/tab, 값을 고르는 거면 radiogroup/radio */
  tabs?: boolean;
  className?: string;
}) {
  const { ref, live, pillStyle } = useSegPill(value);
  return (
    <div
      ref={ref}
      className={cx('ui-seg', live && 'is-live', size === 'sm' && 'is-sm', fill && 'is-fill', className)}
      role={tabs ? 'tablist' : 'radiogroup'} aria-label={label} aria-labelledby={labelledBy}
    >
      <span className="ui-seg-thumb" aria-hidden="true" style={pillStyle} />
      {options.map(o => (
        <button
          key={o.value} type="button" disabled={o.disabled}
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

/** [값, 이름] 짝 목록을 Segmented 옵션으로 */
export const segOptions = <T extends string>(pairs: readonly (readonly [T, ReactNode])[]): SegOption<T>[] => pairs.map(([value, label]) => ({ value, label }));
