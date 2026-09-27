// 관리자 화면 공통 조각 — 칩, 페이지 머리, 빈 상태, 로딩, 오류, 필터 레일.
import { createContext, useContext, type ReactNode } from 'react';
import { Link, useLocation } from '../lib/router';
import type { Duty, Overview, StaffMe } from './api';
import type { Tone } from './labels';

export function Chip({ tone = 'gray', children }: { tone?: Tone; children: ReactNode }) {
  return <span className={`adm-chip adm-t-${tone}`}>{children}</span>;
}

export function StatusChip({ value }: { value: [string, Tone] }) {
  return <Chip tone={value[1]}>{value[0]}</Chip>;
}

export function PageHead({ eyebrow, title, sub, actions }: { eyebrow: string; title: ReactNode; sub?: ReactNode; actions?: ReactNode }) {
  return (
    <div className="adm-head">
      <div>
        <p className="eyebrow">{eyebrow}</p>
        <h1>{title}</h1>
        {sub && <p className="adm-sub">{sub}</p>}
      </div>
      {actions && <div className="adm-head-actions">{actions}</div>}
    </div>
  );
}

export function Empty({ icon = '✓', title, children }: { icon?: string; title: string; children?: ReactNode }) {
  return (
    <div className="adm-empty">
      <div className="adm-empty-icon" aria-hidden="true">{icon}</div>
      <h3>{title}</h3>
      {children && <p>{children}</p>}
    </div>
  );
}

export function Skeleton({ rows = 4 }: { rows?: number }) {
  return (
    <div className="adm-skel" role="status" aria-label="불러오는 중">
      {Array.from({ length: rows }, (_, i) => <span key={i} />)}
    </div>
  );
}

export function ErrorBox({ message, onRetry }: { message: string; onRetry?: () => void }) {
  return (
    <div className="adm-alert is-error adm-alert-row" role="alert">
      <span>{message}</span>
      {onRetry && <button type="button" className="adm-btn danger small" onClick={onRetry}>다시 시도</button>}
    </div>
  );
}

export function Filters<T extends string>({ value, options, onChange, label }: {
  value: T; options: { value: T; label: string; count?: number }[]; onChange: (v: T) => void; label: string;
}) {
  return (
    <div className="adm-filters" role="group" aria-label={label}>
      {options.map(o => (
        <button key={o.value} type="button" className="adm-filter" aria-pressed={o.value === value} onClick={() => onChange(o.value)}>
          {o.label}{o.count != null && o.count > 0 ? ` ${o.count}` : ''}
        </button>
      ))}
    </div>
  );
}

export function Section({ title, meta, action, children }: { title: string; meta?: ReactNode; action?: ReactNode; children: ReactNode }) {
  return (
    <section className="adm-section">
      <div className="adm-section-top">
        <h2>{title}{meta != null && <small>{meta}</small>}</h2>
        {action}
      </div>
      {children}
    </section>
  );
}

/** 이니셜 아이콘 (커버 대신) */
/** 앨범 커버(있으면 실제 이미지) 또는 첫 글자 */
export function Initial({ text, plain, src }: { text: string; plain?: boolean; src?: string | null }) {
  if (src) return <span className="adm-row-icon has-cover" aria-hidden="true"><img src={src} alt="" loading="lazy" decoding="async" /></span>;
  return <span className={`adm-row-icon${plain ? ' plain' : ''}`} aria-hidden="true">{(text || '?').trim().slice(0, 1).toUpperCase()}</span>;
}

// ---------- 스태프 컨텍스트 ----------
interface StaffCtx {
  me: StaffMe;
  counts: Overview | null;
  refreshCounts: () => void;
  can: (d: Duty) => boolean;
}
export const StaffContext = createContext<StaffCtx | null>(null);
export function useStaff(): StaffCtx {
  const v = useContext(StaffContext);
  if (!v) throw new Error('useStaff must be used within AdminApp');
  return v;
}

/** 권한이 없는 작업 버튼 옆에 붙이는 안내 */
export function NoDuty({ duty }: { duty: string }) {
  return <div className="adm-alert">현재 역할로는 <b>{duty}</b> 작업을 할 수 없어요. 조회만 가능해요.</div>;
}

/** 한 메뉴 안의 하위 화면 (발매 심사 | 2차 승인, 배급 현황 | DSP 사양) */
export function SubTabs({ tabs }: { tabs: { to: string; label: string; count?: number }[] }) {
  const loc = useLocation();
  return (
    <div className="adm-filters adm-subtabs" role="tablist">
      {tabs.map(t => (
        <Link key={t.to} to={t.to} role="tab" className="adm-filter" aria-selected={loc.pathname === t.to} aria-pressed={loc.pathname === t.to}>
          {t.label}{t.count ? ` ${t.count}` : ''}
        </Link>
      ))}
    </div>
  );
}
