import { gradientFor } from '../lib/catalog';

/** 발매 커버 — 등록한 커버 이미지가 있으면 이미지, 없으면 ID 기반 그라디언트 */
export function ReleaseCover({ id, src, className = 'cover aq-cover', size }: {
  id: string;
  src?: string;
  className?: string;
  size?: number;
}) {
  const style = size ? { width: size, height: size, flexBasis: size } : undefined;
  if (src) {
    return (
      <span className={`${className} has-image`} aria-hidden="true" style={style}>
        <img src={src} alt="" loading="lazy" decoding="async" />
      </span>
    );
  }
  return (
    <span className={className} aria-hidden="true" style={{ ...style, background: gradientFor(id) }}>♫</span>
  );
}
