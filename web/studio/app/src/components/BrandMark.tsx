// 은행·플랫폼(DSP) 실제 로고. 파일은 public/static/brands/에 두고 lib/brands.ts 목록으로 찾는다.
// - 단색 SVG(브랜드 공식 가이드 기반 simple-icons): 브랜드 색 칸 위에 흰(또는 검은) 표시
// - 공식 앱 아이콘: 칸을 꽉 채운 원본 색 그대로
// - 공식 홈페이지 로고 그림: 흰 칸 안에 여백을 두고
// - 아직 파일이 없으면 브랜드 색 원 + 첫 글자
import { BRAND_ASSETS } from '../lib/brands';

const darkInk = (hex: string) => {
  const h = hex.replace('#', '');
  const [r, g, b] = [0, 2, 4].map(i => parseInt(h.slice(i, i + 2), 16) / 255);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b > 0.62;
};

export function BrandMark({ id, label, color = '#3B63F3', size = 32, className }: {
  id: string; label: string; color?: string; size?: number; className?: string;
}) {
  const a = BRAND_ASSETS[id];
  const base = `${import.meta.env.BASE_URL}static/brands/`;
  const style = { width: size, height: size, '--brand': a?.color ?? color } as React.CSSProperties;
  const cls = `aq-brand${className ? ` ${className}` : ''}`;
  if (a?.kind === 'glyph') {
    return (
      <span className={`${cls} is-glyph${darkInk(a.color ?? color) ? ' is-ink' : ''}`} style={style} aria-hidden="true">
        {/* CSS 마스크 대신 그림 + 색 필터 — 아이폰 사파리의 '전체 페이지' 캡처(PDF)는 마스크를 버려 로고가 검은 덩어리로 찍혔다 */}
        <img src={`${base}${a.file}`} alt="" loading="lazy" decoding="async" draggable={false} />
      </span>
    );
  }
  if (a) {
    return (
      <span className={`${cls} ${a.kind === 'app' ? 'is-app' : 'is-image'}`} style={style} aria-hidden="true">
        <img src={`${base}${a.file}`} alt="" loading="lazy" decoding="async" draggable={false} />
      </span>
    );
  }
  return <span className={`${cls} is-letter`} style={style} aria-hidden="true">{label.trim().slice(0, 1)}</span>;
}
