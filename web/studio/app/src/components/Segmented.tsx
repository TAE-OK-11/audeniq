// 스튜디오 화면용 고르기 토글 — 디자인 시스템 Segmented를 칸을 같은 폭으로 채워서 쓴다
import type { ReactNode } from 'react';
import { Segmented as UISegmented } from '../ui';

export function Segmented<T extends string>({ value, options, onChange, label, labelledBy, className, tabs = false }: {
  value: T;
  options: readonly { value: T; label: ReactNode }[];
  onChange: (v: T) => void;
  label?: string;
  labelledBy?: string;
  className?: string;
  /** 아래 내용을 바꾸는 탭이면 tablist/tab, 값을 고르는 거면 radiogroup/radio */
  tabs?: boolean;
}) {
  return <UISegmented value={value} options={options} onChange={onChange} label={label} labelledBy={labelledBy} className={className} tabs={tabs} fill />;
}
