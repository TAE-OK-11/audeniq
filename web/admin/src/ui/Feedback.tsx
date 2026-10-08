// 알림 상자 · 빈 상태 · 불러오는 중
import type { ReactNode } from 'react';
import { CheckIcon } from '../components/Check';
import { IconTile } from './Media';
import { cx, type Tone } from './tone';

export type CalloutTone = 'info' | 'success' | 'warn' | 'danger' | 'neutral';

/** 안내·경고·오류·완료 상자. action을 주면 오른쪽에 버튼이 붙는 한 줄 */
export function Callout({ tone = 'info', action, role, className, children }: { tone?: CalloutTone; action?: ReactNode; role?: 'alert' | 'status'; className?: string; children: ReactNode }) {
  return (
    <div className={cx('ui-callout', tone !== 'info' && `is-${tone}`, action != null && 'is-row', className)} role={role ?? (tone === 'danger' ? 'alert' : undefined)}>
      {action != null ? <><span>{children}</span>{action}</> : children}
    </div>
  );
}

/** 목록이 비었을 때 — 아이콘 · 제목 · 설명 · 다음 행동 */
export function EmptyState({ icon = <CheckIcon size={22} />, tone = 'blue', title, children, actions, compact, as: H = 'h3' }: {
  icon?: ReactNode; tone?: Tone; title: ReactNode; children?: ReactNode; actions?: ReactNode; compact?: boolean; as?: 'h2' | 'h3';
}) {
  return (
    <div className={cx('ui-empty', compact && 'is-compact')}>
      {icon && <IconTile tone={tone} size="lg">{icon}</IconTile>}
      <H>{title}</H>
      {children && <p>{children}</p>}
      {actions && <div className="ui-actions">{actions}</div>}
    </div>
  );
}

/** 목록 줄 자리를 미리 보여 주는 로딩 */
export function SkeletonList({ rows = 4 }: { rows?: number }) {
  return (
    <div className="ui-skel-rows" role="status" aria-label="불러오는 중">
      {Array.from({ length: rows }, (_, i) => <span key={i} style={{ animationDelay: `${i * 70}ms` }} />)}
    </div>
  );
}
