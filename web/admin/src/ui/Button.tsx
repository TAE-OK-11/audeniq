// 버튼 — 화면 어디서든 같은 높이·모서리·색 단계 (ui.css '버튼')
import type { ButtonHTMLAttributes, ReactNode } from 'react';
import { cx } from './tone';

export type ButtonVariant = 'primary' | 'soft' | 'neutral' | 'ghost' | 'white' | 'warn' | 'warn-solid' | 'danger' | 'danger-solid' | 'text';
export type ButtonSize = 'xs' | 'sm' | 'md' | 'lg';

export interface ButtonLook {
  /** primary: 화면의 주 실행 하나 · soft(기본): 보조 실행 · neutral/ghost: 덜 중요한 실행 · danger: 되돌리기 어려운 실행
   *  (옅은 카드·어두운 패널 위에서는 면에 맞춰 색이 자동으로 바뀐다) */
  variant?: ButtonVariant;
  size?: ButtonSize;
  block?: boolean;
  busy?: boolean;
  icon?: boolean;
}

/** <a>·<Link>에 버튼 모양을 입힐 때 */
export function buttonClass({ variant = 'soft', size = 'md', block, busy, icon }: ButtonLook = {}, extra?: string) {
  return cx('ui-btn', variant !== 'soft' && `is-${variant}`, size !== 'md' && `is-${size}`, block && 'is-block', busy && 'is-busy', icon && 'is-icon', extra);
}

export function Button({ variant, size, block, busy, icon, className, type = 'button', children, disabled, ...rest }: ButtonLook & ButtonHTMLAttributes<HTMLButtonElement> & { children?: ReactNode }) {
  return (
    <button
      type={type}
      className={buttonClass({ variant, size, block, busy, icon }, className)}
      disabled={disabled || busy}
      aria-busy={busy || undefined}
      {...rest}
    >
      {children}
    </button>
  );
}

/** 버튼 끝 화살표 (주 실행 버튼 안) */
export function ButtonArrow() {
  return (
    <span className="ui-btn-arrow" aria-hidden="true">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round"><path d="M5 12h14M13 6l6 6-6 6" /></svg>
    </span>
  );
}
