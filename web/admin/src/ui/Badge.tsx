// 상태 배지 · 정보 태그 · 숫자 배지
import type { ReactNode } from 'react';
import { cx, type Tone } from './tone';

/** 진행 상태·결과 (검토 중, 발매 완료, 2시간 지남 …) — 둥근 알약 */
export function Badge({ tone = 'gray', dot, size, className, title, children }: {
  tone?: Tone; dot?: boolean; size?: 'md' | 'lg'; className?: string; title?: string; children: ReactNode;
}) {
  return <span className={cx('ui-badge', `is-${tone}`, dot && 'is-dot', size === 'lg' && 'is-lg', className)} title={title}>{children}</span>;
}

/** 종류·번호·플랫폼 같은 정보 이름표 — 각진 작은 칸. mono: 업무 번호·코드 */
export function Tag({ tone, mono, white, className, title, children }: {
  tone?: Tone; mono?: boolean; white?: boolean; className?: string; title?: string; children: ReactNode;
}) {
  return <span className={cx('ui-tag', tone && `is-${tone}`, mono && 'is-mono', white && 'is-white', className)} title={title}>{children}</span>;
}

/** 탭·메뉴·필터 옆 개수. hot: 처리할 일이 있음(파랑) · alert: 읽지 않은 알림(빨강) */
export function Count({ value, tone, max = 99, label }: { value: number; tone?: 'hot' | 'soft' | 'alert'; max?: number; label?: string }) {
  return <span className={cx('ui-count', tone && `is-${tone}`)} aria-label={label}>{value > max ? `${max}+` : value}</span>;
}
