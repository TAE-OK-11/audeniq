// 아바타 · 앨범 커버 · 아이콘 칸
import type { ReactNode } from 'react';
import { Glyph } from '../components/Glyph';
import { apiUrl } from '../api/http';
import { gradientFor } from './cover';
import { cx, type Tone } from './tone';

const initial = (s: string | null | undefined) => (s || '?').trim().slice(0, 1).toUpperCase() || '?';

/** 사람 — 이름 첫 글자. me: 나(파란 면) · empty: 담당자 없음 */
export function Avatar({ name, size = 'md', me, empty, title }: { name?: string | null; size?: 'xs' | 'sm' | 'md' | 'lg' | 'xl'; me?: boolean; empty?: boolean; title?: string }) {
  return (
    <span className={cx('ui-avatar', size !== 'md' && `is-${size}`, me && 'is-me', empty && 'is-empty')} aria-hidden="true" title={title}>
      {empty ? '–' : me ? '나' : initial(name)}
    </span>
  );
}

/** 발매 커버 — 이미지가 있으면 이미지, 없으면 발매마다 다른 그라데이션 위 음표 (스튜디오와 같은 모양).
 *  plain: 서류처럼 발매가 아닌 항목 — 연파랑 면 위 첫 글자 */
export function Cover({ id, text, src, size = 'md', plain }: { id?: string; text: string; src?: string | null; size?: 'md' | 'lg' | 'xl'; plain?: boolean }) {
  const cls = cx('ui-cover', size !== 'md' && `is-${size}`, src && 'has-image', plain && 'is-plain');
  if (src) return <span className={cls} aria-hidden="true"><img src={apiUrl(src)} alt="" loading="lazy" decoding="async" /></span>;
  if (plain) return <span className={cls} aria-hidden="true">{initial(text)}</span>;
  return <span className={cls} aria-hidden="true" style={{ background: gradientFor(id || text) }}><Glyph name="music" className="ui-cover-glyph" /></span>;
}

/** 아이콘 칸 — 바로가기·숫자 칸·빈 상태 */
export function IconTile({ children, tone = 'blue', size = 'md' }: { children: ReactNode; tone?: Tone; size?: 'sm' | 'md' | 'lg' }) {
  return <span className={cx('ui-icon', tone !== 'blue' && `is-${tone}`, size !== 'md' && `is-${size}`)} aria-hidden="true">{children}</span>;
}
