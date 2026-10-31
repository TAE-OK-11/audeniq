// AUDENIQ 정보 카드 — 작은 제목 → 한 문장 → 이름·값 줄(또는 누르는 줄) → 맨 아래 작은 글자 동작.
// 퍼블리싱 곡 화면에서 만든 틀을 배급(발매 상세) 등 다른 화면도 같이 쓴다. 스타일: styles/enhance.css .aq-icard
import type { ReactNode } from 'react';
import { Link } from '../lib/router';
import { Glyph, type GlyphName } from './Glyph';

export function InfoCard({ cap, title, lead, id, children, className }: { cap: string; title: ReactNode; lead?: ReactNode; id?: string; children?: ReactNode; className?: string }) {
  return (
    <section className={`aq-relinfo-card aq-icard${className ? ` ${className}` : ''}`} aria-labelledby={id}>
      <p className="aq-icard-cap" id={id}>{cap}</p>
      <h2 className="aq-icard-title">{title}</h2>
      {lead && <p className="aq-icard-lead">{lead}</p>}
      {children}
    </section>
  );
}

/** 줄 묶음 — 연한 판 위에 줄 사이 구분선 */
export function InfoRows({ children }: { children: ReactNode }) {
  return <ul className="aq-kvs">{children}</ul>;
}

/** 이름 왼쪽(흐리게, 아래 작은 설명) · 값 오른쪽(굵게) */
export function InfoRow({ k, sub, v }: { k: ReactNode; sub?: ReactNode; v?: ReactNode }) {
  return <li className="aq-kv"><span>{k}{sub && <small>{sub}</small>}</span>{v !== undefined && <b>{v}</b>}</li>;
}

/** 누르는 줄 — 오른쪽에 '보기 >' 같은 짧은 말 */
export function InfoLink({ k, sub, go = '보기', to, onClick, strong, icon = 'chevron-right' }: {
  k: ReactNode; sub?: ReactNode; go?: string; to?: string; onClick?: () => void; strong?: boolean; icon?: GlyphName;
}) {
  const body = <><span>{k}{sub && <small>{sub}</small>}</span><b className="aq-kv-go">{go}<Glyph name={icon} size={15} /></b></>;
  const cls = `aq-kv is-link${strong ? ' is-strong' : ''}`;
  return <li>{to ? <Link to={to} className={cls}>{body}</Link> : <button type="button" className={cls} onClick={onClick}>{body}</button>}</li>;
}
