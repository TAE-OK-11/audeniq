// 숫자 칸 · 항목·값 · 면
import type { CSSProperties, ReactNode } from 'react';
import { Link } from '../lib/router';
import { cx } from './tone';

export type StatTone = 'alert' | 'warn' | 'muted' | 'active';

/** 숫자 칸 — [이름표 · 아이콘] / 큰 숫자 / 설명. to·onClick이 있으면 눌러서 이동 */
export function StatTile({ label, value, unit, hint, icon, tone, compact, to, onClick, pressed, ariaLabel }: {
  label: ReactNode; value: ReactNode; unit?: ReactNode; hint?: ReactNode; icon?: ReactNode; tone?: StatTone; compact?: boolean;
  to?: string; onClick?: () => void; pressed?: boolean; ariaLabel?: string;
}) {
  const cls = cx('ui-stat', compact && 'is-compact', tone && `is-${tone}`);
  const body = (
    <>
      <span className="ui-stat-top"><span className="ui-stat-label">{label}</span>{icon}</span>
      <span>
        <strong className="ui-stat-value">{value}{unit && <small>{unit}</small>}</strong>
        {hint && <small className="ui-stat-hint">{hint}</small>}
      </span>
    </>
  );
  if (to) return <Link to={to} className={cls} aria-label={ariaLabel}>{body}</Link>;
  if (onClick) return <button type="button" className={cls} onClick={onClick} aria-pressed={pressed} aria-label={ariaLabel}>{body}</button>;
  return <div className={cls} aria-label={ariaLabel}>{body}</div>;
}

export function StatGrid({ cols = 4, children, className }: { cols?: number; children: ReactNode; className?: string }) {
  return <div className={cx('ui-stats', className)} style={{ '--cols': cols } as CSSProperties}>{children}</div>;
}

/** 항목·값 목록 (dl) */
export function KeyValue({ items, cols = 3, className }: { items: readonly (readonly [ReactNode, ReactNode] | null | false)[]; cols?: number; className?: string }) {
  return (
    <dl className={cx('ui-kv', className)} style={{ '--cols': cols } as CSSProperties}>
      {items.filter(Boolean).map((it, i) => {
        const [k, v] = it as readonly [ReactNode, ReactNode];
        return <div key={i}><dt>{k}</dt><dd>{v}</dd></div>;
      })}
    </dl>
  );
}

/** 옅은 파란 면 카드. white: 옅은 면 위에 올릴 흰 카드 · tight: 작은 여백 */
export function Card({ children, white, tight, className, as: Tag = 'div' }: { children: ReactNode; white?: boolean; tight?: boolean; className?: string; as?: 'div' | 'section' | 'article' }) {
  return <Tag className={cx('ui-card', white && 'is-white', tight && 'is-tight', className)}>{children}</Tag>;
}
