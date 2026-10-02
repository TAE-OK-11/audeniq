// AUDENIQ 아이콘 — 기기·글꼴마다 다르게 그려지는 글자 기호(✎ ▤ ♫ ₩ ✉ ↗ → ← × 이모지 …) 대신 쓰는 SVG.
// 모두 24×24, 선 두께 2, 둥근 끝, currentColor.
import type { ReactNode } from 'react';

const P: Record<string, ReactNode> = {
  pencil: <><path d="M4 20h4L19 9a2.8 2.8 0 0 0-4-4L4 16v4Z" /><path d="m13.5 6.5 4 4" /></>,
  doc: <><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8Z" /><path d="M14 3v5h5M9 13h6M9 17h4" /></>,
  music: <><path d="M9 18V6l11-2v12" /><circle cx="6.5" cy="18" r="2.5" /><circle cx="17.5" cy="16" r="2.5" /></>,
  won: <><path d="m4 6 3.5 12L12 8l4.5 10L20 6" /><path d="M3 11h18" /></>,
  mail: <><rect x="3" y="5" width="18" height="14" rx="3" /><path d="m4 7 8 6 8-6" /></>,
  user: <><circle cx="12" cy="8" r="4" /><path d="M4 20a8 8 0 0 1 16 0" /></>,
  dot: <circle cx="12" cy="12" r="3" fill="currentColor" />,
  close: <path d="M6 6l12 12M18 6 6 18" />,
  plus: <path d="M12 5v14M5 12h14" />,
  'arrow-up-right': <path d="M7 17 17 7M9 7h8v8" />,
  'arrow-right': <path d="M5 12h14M13 6l6 6-6 6" />,
  'arrow-left': <path d="M19 12H5M11 6l-6 6 6 6" />,
  'chevron-right': <path d="m9 6 6 6-6 6" />,
  download: <path d="M12 4v11M7 10l5 5 5-5M5 20h14" />,
  lock: <><rect x="4" y="10" width="16" height="11" rx="3" /><path d="M8 10V7a4 4 0 0 1 8 0v3" /></>,
  truck: <><path d="M3 7h11v10H3zM14 10h4l3 3v4h-7" /><circle cx="7" cy="18" r="2" /><circle cx="17" cy="18" r="2" /></>,
  alert: <><path d="M12 7v6" /><circle cx="12" cy="17" r="1" fill="currentColor" /></>,
  bell: <><path d="M6 8a6 6 0 1 1 12 0c0 7 3 9 3 9H3s3-2 3-9" /><path d="M10.3 21a1.94 1.94 0 0 0 3.4 0" /></>,
  warning: <><path d="M10.3 3.9 2.6 17.3A2 2 0 0 0 4.3 20h15.4a2 2 0 0 0 1.7-2.7L13.7 3.9a2 2 0 0 0-3.4 0Z" /><path d="M12 9v4" /><circle cx="12" cy="16.5" r="1" fill="currentColor" /></>,
  sign: <><path d="M12 20h9" /><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z" /></>,
  release: <><rect x="3" y="3" width="18" height="18" rx="4" /><circle cx="12" cy="12" r="4" /><circle cx="12" cy="12" r="1" fill="currentColor" /></>,
  chart: <path d="M4 20V4M4 20h16M8 16v-4M13 16V8M18 16v-7" />,
  home: <><path d="M3 10.5 12 3l9 7.5V20a1 1 0 0 1-1 1h-5v-6H9v6H4a1 1 0 0 1-1-1Z" /></>,
  review: <><rect x="4" y="3" width="16" height="18" rx="3" /><path d="m9 12 2 2 4-4" /></>,
  approval: <><circle cx="9" cy="7" r="3.5" /><path d="M3 21v-1a6 6 0 0 1 6-6h2" /><path d="m14 18 2.5 2.5L21 16" /></>,
  inquiry: <path d="M21 12a8 8 0 0 1-11.5 7.2L4 21l1.8-5.5A8 8 0 1 1 21 12Z" />,
  globe: <><circle cx="12" cy="12" r="9" /><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18" /></>,
  card: <><rect x="3" y="6" width="18" height="13" rx="3" /><path d="M3 10h18M7 15h3" /></>,
};

export type GlyphName = keyof typeof P;

export function Glyph({ name, size = 16, className }: { name: string; size?: number; className?: string }) {
  return (
    <svg
      className={`aq-glyph${className ? ` ${className}` : ''}`} viewBox="0 0 24 24" width={size} height={size}
      fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"
      aria-hidden="true" focusable="false"
    >
      {P[name] ?? P.dot}
    </svg>
  );
}
