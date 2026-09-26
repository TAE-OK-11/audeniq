// 오류·점검 화면 — 앱 안의 오류 카드(.empty-page)와 같은 가운데 정렬 연파랑 카드에
// 아이콘 배지·오류 코드 칩·제목·안내·버튼. 스튜디오 안(헤더 있음)과 앱 전체(로고만)에서 같은 모양.
import type { ReactNode } from 'react';

export type ErrorKind = 'not-found' | 'server' | 'crash' | 'update' | 'offline' | 'maintenance' | 'device';

interface Action {
  label: string;
  onClick?: () => void;
  href?: string;
  primary?: boolean;
}

interface Props {
  kind: ErrorKind;
  /** 제목 위 칩에 보여 줄 코드 (예: 404, 502) */
  code?: string;
  eyebrow?: string;
  title: ReactNode;
  description?: ReactNode;
  actions?: Action[];
  /** 문의할 때 알려 줄 정보 (오류 코드·시각·참조 ID) */
  meta?: string[];
  /** 앱 전체를 덮는 화면 (로고 표시, 화면 높이 가득) */
  fullPage?: boolean;
  children?: ReactNode;
}

const ICONS: Record<ErrorKind, ReactNode> = {
  'not-found': <><circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" /><path d="M11 8v3" /><path d="M11 14h.01" /></>,
  server: <><rect x="3" y="4" width="18" height="7" rx="2" /><rect x="3" y="13" width="18" height="7" rx="2" /><path d="M7 7.5h.01M7 16.5h.01" /><path d="M16 16.5h2" /></>,
  crash: <><path d="M12 3 2.5 20h19Z" /><path d="M12 10v4" /><path d="M12 17h.01" /></>,
  update: <><path d="M21 12a9 9 0 1 1-2.64-6.36" /><path d="M21 4v5h-5" /></>,
  offline: <><path d="M2 8.8a15 15 0 0 1 4.2-2.6" /><path d="M10.7 5.1A15 15 0 0 1 22 8.8" /><path d="M5 12.5a10 10 0 0 1 5.2-2.7" /><path d="M16.9 10.7A10 10 0 0 1 19 12.5" /><path d="M8.5 16.1a5 5 0 0 1 7 0" /><path d="M12 20h.01" /><path d="m3 3 18 18" /></>,
  maintenance: <><path d="M14.7 6.3a4 4 0 0 0-5.4 5.4L3 18v3h3l6.3-6.3a4 4 0 0 0 5.4-5.4l-2.6 2.6-2.4-.6-.6-2.4Z" /></>,
  device: <><rect x="5" y="2" width="14" height="20" rx="3" /><path d="M12 18h.01" /><path d="M12 7v5" /></>,
};

export function ErrorIcon({ kind }: { kind: ErrorKind }) {
  return (
    <svg viewBox="0 0 24 24" width="28" height="28" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      {ICONS[kind]}
    </svg>
  );
}

export function ErrorScreen({ kind, code, eyebrow, title, description, actions = [], meta, fullPage, children }: Props) {
  const body = (
    <section className={`aq-errscreen is-${kind}`} role={kind === 'maintenance' || kind === 'update' ? 'status' : 'alert'}>
      <span className="aq-errscreen-icon" aria-hidden="true"><ErrorIcon kind={kind} /></span>
      {(code || eyebrow) && (
        <p className="aq-errscreen-eyebrow">
          {code && <span className="aq-errscreen-code">{code}</span>}
          {eyebrow}
        </p>
      )}
      <h1>{title}</h1>
      {description && <div className="aq-errscreen-desc">{description}</div>}
      {children}
      {actions.length > 0 && (
        <div className="aq-errscreen-actions">
          {actions.map(a => (a.href
            ? <a key={a.label} className={`button${a.primary ? '' : ' secondary'}`} href={a.href}>{a.label}</a>
            : <button key={a.label} type="button" className={`button${a.primary ? '' : ' secondary'}`} onClick={a.onClick}>{a.label}</button>))}
        </div>
      )}
      {meta && meta.length > 0 && <p className="aq-errscreen-meta">{meta.join(' · ')}</p>}
    </section>
  );
  if (!fullPage) return body;
  return (
    <div className="aq-errpage">
      <a className="aq-errpage-brand" href={import.meta.env.BASE_URL} aria-label="AUDENIQ STUDIO 홈">
        <img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" />
        <span>STUDIO</span>
      </a>
      {body}
    </div>
  );
}

/** 문의 시 알려 줄 짧은 참조 ID와 시각 */
export function incidentMeta(code: string): string[] {
  const now = new Date();
  const pad = (n: number) => String(n).padStart(2, '0');
  const at = `${now.getFullYear()}.${pad(now.getMonth() + 1)}.${pad(now.getDate())} ${pad(now.getHours())}:${pad(now.getMinutes())}`;
  const ref = Math.random().toString(36).slice(2, 8).toUpperCase();
  return [`오류 코드 ${code}`, at, `참조 ${ref}`];
}
