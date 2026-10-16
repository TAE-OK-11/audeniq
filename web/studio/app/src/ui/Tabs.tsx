// 탭 — 화면 안의 하위 화면. 고른 탭 아래로 파란 밑줄이 미끄러진다.
import type { ReactNode } from 'react';
import { Link, useLocation } from '../lib/router';
import { useSegPill } from '../hooks/useSegPill';
import { cx } from './tone';

export interface TabItem<T extends string = string> { value: T; label: ReactNode; count?: number | null; id?: string; controls?: string }

function TabCount({ n }: { n?: number | null }) {
  return n ? <span className="ui-count">{n}</span> : null;
}

/** 같은 화면 안에서 내용을 바꾸는 탭 */
export function Tabs<T extends string>({ value, items, onChange, label, className, onIntent }: {
  value: T; items: readonly TabItem<T>[]; onChange: (v: T) => void; label: string; className?: string;
  /** 누르기 직전(손가락이 닿거나 마우스가 올라갈 때) — 그 탭의 데이터를 미리 받아 두면 바꿀 때 빈 화면이 없다 */
  onIntent?: (v: T) => void;
}) {
  const { ref, live, pillStyle } = useSegPill(value);
  return (
    <div ref={ref} className={cx('ui-tabs', live && 'is-live', className)} role="tablist" aria-label={label}>
      <span className="ui-tabs-bar" aria-hidden="true" style={pillStyle} />
      {items.map(t => (
        <button
          key={t.value} id={t.id} type="button" role="tab" className="ui-tab"
          aria-selected={t.value === value} aria-controls={t.controls}
          onClick={() => onChange(t.value)}
          onPointerDown={onIntent && (() => onIntent(t.value))}
          onPointerEnter={onIntent && (() => onIntent(t.value))}
        >
          {t.label}<TabCount n={t.count} />
        </button>
      ))}
    </div>
  );
}

/** 주소가 바뀌는 탭 (관리자 하위 메뉴) — 화면이 새로 그려져도 이전 탭 자리에서 밑줄이 출발한다 */
export function LinkTabs({ tabs, label = '하위 화면' }: { tabs: readonly { to: string; label: ReactNode; count?: number | null }[]; label?: string }) {
  const loc = useLocation();
  const on = (to: string) => loc.pathname === to || loc.pathname.startsWith(`${to}/`);
  const { ref, live, pillStyle } = useSegPill(loc.pathname, `tabs:${tabs.map(t => t.to).join('|')}`);
  return (
    <div ref={ref} className={cx('ui-tabs', live && 'is-live')} role="tablist" aria-label={label}>
      <span className="ui-tabs-bar" aria-hidden="true" style={pillStyle} />
      {tabs.map(t => (
        <Link key={t.to} to={t.to} role="tab" className="ui-tab" aria-selected={on(t.to)}>
          {t.label}<TabCount n={t.count} />
        </Link>
      ))}
    </div>
  );
}
