// 입력 묶음 — 이름표 · 칸 · 도움말/오류. 칸은 ui-input / ui-select / ui-textarea 클래스.
import type { ReactNode } from 'react';
import { cx } from './tone';

export function Field({ label, htmlFor, required, help, error, warn, children, className }: {
  label?: ReactNode; htmlFor?: string; required?: boolean; help?: ReactNode; error?: ReactNode; warn?: ReactNode; children: ReactNode; className?: string;
}) {
  return (
    <div className={cx('ui-field', className)}>
      {label && <label htmlFor={htmlFor}>{label}{required && <span className="ui-required" aria-hidden="true">*</span>}</label>}
      {children}
      {error ? <small className="ui-field-help is-error" role="alert">{error}</small>
        : warn ? <small className="ui-field-help is-warn">{warn}</small>
        : help ? <small className="ui-field-help">{help}</small> : null}
    </div>
  );
}

/** 입력칸 클래스 (크기) */
export const inputClass = (size?: 'sm' | 'md' | 'lg', extra?: string) => cx('ui-input', size && size !== 'md' && `is-${size}`, extra);
export const selectClass = (size?: 'sm' | 'md' | 'lg', extra?: string) => cx('ui-select', size && size !== 'md' && `is-${size}`, extra);
