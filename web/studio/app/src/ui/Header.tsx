// 화면 머리 · 섹션 머리
import type { ReactNode } from 'react';
import { cx } from './tone';

/** 화면 맨 위: 머리표 · 제목 · 설명 · 실행 버튼 */
export function PageHeader({ eyebrow, title, sub, actions, back, className }: {
  eyebrow?: ReactNode; title: ReactNode; sub?: ReactNode; actions?: ReactNode; back?: ReactNode; className?: string;
}) {
  return (
    <header className={cx('ui-page-head', className)}>
      <div>
        {back}
        {eyebrow && <p className="ui-eyebrow">{eyebrow}</p>}
        <h1>{title}</h1>
        {sub && <p className="ui-page-sub">{sub}</p>}
      </div>
      {actions && <div className="ui-page-actions">{actions}</div>}
    </header>
  );
}

/** 섹션 제목 줄: 제목 · 작은 보조 정보 · 오른쪽 실행 */
export function SectionHeader({ title, meta, action, as: H = 'h2', id }: { title: ReactNode; meta?: ReactNode; action?: ReactNode; as?: 'h2' | 'h3'; id?: string }) {
  return (
    <div className="ui-section-head">
      <H id={id}>{title}{meta != null && <small>{meta}</small>}</H>
      {action}
    </div>
  );
}

export function Section({ title, meta, action, children, className }: { title: ReactNode; meta?: ReactNode; action?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <section className={cx('ui-section', className)}>
      <SectionHeader title={title} meta={meta} action={action} />
      {children}
    </section>
  );
}
