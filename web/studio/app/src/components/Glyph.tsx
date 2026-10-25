// AUDENIQ 아이콘 — 기기·글꼴마다 다르게 그려지는 글자 기호(✎ ▤ ♫ ₩ ✉ ↗ → ← × 이모지 …) 대신 쓰는 SVG.
// 모두 24×24, 선 두께 2, 둥근 끝, currentColor.
import type { ReactNode } from 'react';

const P: Record<string, ReactNode> = {
  pencil: <><path d="M4 20h4L19 9a2.8 2.8 0 0 0-4-4L4 16v4Z" /><path d="m13.5 6.5 4 4" /></>,
  doc: <><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8Z" /><path d="M14 3v5h5M9 13h6M9 17h4" /></>,
  music: <><path d="M9 18V6l11-2v12" /><circle cx="6.5" cy="18" r="2.5" /><circle cx="17.5" cy="16" r="2.5" /></>,
  won: <><path d="m5.5 6 3 12L12 8.5l3.5 9.5 3-12" /><path d="M3.8 11.6h16.4" /></>,
  // 화폐 — 글꼴마다 모양이 다른 ₩ $ € ¥ £ 대신 같은 선 굵기의 AUDENIQ 기호
  krw: <><path d="m5.5 6 3 12L12 8.5l3.5 9.5 3-12" /><path d="M3.8 11.6h16.4" /></>,
  usd: <><path d="M16.2 8.2c-.6-1.5-2.2-2.5-4.2-2.5-2.4 0-4 1.3-4 3.1 0 4.3 8.4 2.5 8.4 6.6 0 1.9-1.8 3.1-4.4 3.1-2.2 0-3.8-1-4.4-2.5" /><path d="M12 3.5v17" /></>,
  eur: <><path d="M17.5 7.2A6.6 6.6 0 1 0 17.5 16.8" /><path d="M4 10.3h9M4 13.7h9" /></>,
  jpy: <><path d="m6.5 4.5 5.5 7.6 5.5-7.6" /><path d="M12 12.1v7.4M7.5 13h9M7.5 16.2h9" /></>,
  gbp: <><path d="M16.2 7.6a3.5 3.5 0 0 0-7 .2c0 3 1.4 4.8.8 8.2-.3 1.7-1.2 3-2.5 3.8h9.8" /><path d="M6.8 12.6h7" /></>,
  mail: <><rect x="3" y="5" width="18" height="14" rx="3" /><path d="m4 7 8 6 8-6" /></>,
  user: <><circle cx="12" cy="8" r="4" /><path d="M4 20a8 8 0 0 1 16 0" /></>,
  dot: <circle cx="12" cy="12" r="3" fill="currentColor" />,
  close: <path d="M6 6l12 12M18 6 6 18" />,
  plus: <path d="M12 5v14M5 12h14" />,
  'arrow-up-right': <path d="M7 17 17 7M9 7h8v8" />,
  'arrow-up': <path d="M12 19V5M6 11l6-6 6 6" />,
  'arrow-right': <path d="M5 12h14M13 6l6 6-6 6" />,
  'arrow-left': <path d="M19 12H5M11 6l-6 6 6 6" />,
  'chevron-right': <path d="m9 6 6 6-6 6" />,
  'chevron-left': <path d="m15 6-6 6 6 6" />,
  'chevron-down': <path d="m6 9 6 6 6-6" />,
  check: <path d="M5 12.5 9.5 17 19 7.5" />,
  minus: <path d="M5 12h14" />,
  undo: <><path d="M9 14 4 9l5-5" /><path d="M4 9h10.5a5.5 5.5 0 0 1 0 11H11" /></>,
  redo: <><path d="m15 14 5-5-5-5" /><path d="M20 9H9.5a5.5 5.5 0 0 0 0 11H13" /></>,
  info: <><circle cx="12" cy="12" r="9" /><path d="M12 11v5" /><circle cx="12" cy="7.8" r="1" fill="currentColor" /></>,
  stop: <><circle cx="12" cy="12" r="9" /><path d="M8 12h8" /></>,
  checklist: <><rect x="3.5" y="4" width="6" height="6" rx="1.5" /><path d="m5 7 1.2 1.2L8.4 6" /><rect x="3.5" y="14" width="6" height="6" rx="1.5" /><path d="M13 7h7.5M13 17h7.5" /></>,
  link: <><path d="M10 14a4.5 4.5 0 0 0 6.4 0l3-3a4.5 4.5 0 0 0-6.4-6.4l-1 1" /><path d="M14 10a4.5 4.5 0 0 0-6.4 0l-3 3a4.5 4.5 0 0 0 6.4 6.4l1-1" /></>,
  image: <><rect x="3" y="4" width="18" height="16" rx="3" /><circle cx="9" cy="10" r="2" /><path d="m21 16-5-5-9 9" /></>,
  quote: <><path d="M10 6.5C7 7.3 5 9.7 5 13v4.5h4.5V13H6.6" /><path d="M19 6.5c-3 .8-5 3.2-5 6.5v4.5h4.5V13h-2.9" /></>,
  table: <><rect x="3" y="4" width="18" height="16" rx="3" /><path d="M3 10h18M3 15h18M10 4v16" /></>,
  heading: <path d="M6 5v14M18 5v14M6 12h12" />,
  list: <path d="M9 6h11M9 12h11M9 18h11M4.5 6h.01M4.5 12h.01M4.5 18h.01" />,
  star: <path d="m12 3.5 2.6 5.3 5.9.9-4.3 4.1 1 5.8-5.2-2.7-5.2 2.7 1-5.8-4.3-4.1 5.9-.9Z" />,
  disc: <><circle cx="12" cy="12" r="9" /><circle cx="12" cy="12" r="2.5" /></>,
  scale: <><path d="M12 4v16M8 20h8M5 7h14" /><path d="m5 7-2.5 6a3 3 0 0 0 5 0Zm14 0-2.5 6a3 3 0 0 0 5 0Z" /></>,
  siren: <><path d="M7 17v-5a5 5 0 0 1 10 0v5" /><path d="M5 17h14v3H5zM12 3v2M4.5 6l1.4 1.4M19.5 6l-1.4 1.4" /></>,
  book: <><path d="M5 4.5A1.5 1.5 0 0 1 6.5 3H19v15H6.5A1.5 1.5 0 0 0 5 19.5Z" /><path d="M5 19.5A1.5 1.5 0 0 0 6.5 21H19" /></>,
  antenna: <><path d="M12 12v9" /><circle cx="12" cy="10" r="2" /><path d="M7.8 5.8a6 6 0 0 0 0 8.4M16.2 5.8a6 6 0 0 1 0 8.4M5 3a10 10 0 0 0 0 14M19 3a10 10 0 0 1 0 14" /></>,
  receipt: <><path d="M6 3h12v18l-3-2-3 2-3-2-3 2Z" /><path d="M9 8h6M9 12h6M9 16h3" /></>,
  template: <><rect x="4" y="3" width="16" height="18" rx="3" /><path d="M8 8h8M8 12h8M8 16h5" /></>,
  download: <path d="M12 4v11M7 10l5 5 5-5M5 20h14" />,
  // 디자인 시스템 — 겹친 면
  qr: <><rect x="4" y="4" width="6" height="6" rx="1.2" /><rect x="14" y="4" width="6" height="6" rx="1.2" /><rect x="4" y="14" width="6" height="6" rx="1.2" /><path d="M14 14h2v2M20 14v2M14 20h2M18 18h2v2" /></>,
  layers: <><path d="m12 3 9 5-9 5-9-5 9-5Z" /><path d="m3 13 9 5 9-5" /></>,
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
  // 업무(Work) 모듈
  task: <><rect x="4" y="4" width="16" height="16" rx="4" /><path d="m8.5 12 2.5 2.5 4.5-5" /></>,
  at: <><circle cx="12" cy="12" r="3.6" /><path d="M15.6 12v1.4a2.6 2.6 0 0 0 5.2 0V12a8.8 8.8 0 1 0-3.5 7" /></>,
  calendar: <><rect x="3.5" y="5" width="17" height="15.5" rx="3" /><path d="M3.5 10h17M8 3v4M16 3v4" /></>,
  clock: <><circle cx="12" cy="12" r="9" /><path d="M12 7v5l3.2 2" /></>,
  users: <><circle cx="9" cy="8" r="3.5" /><path d="M2.5 20a6.5 6.5 0 0 1 13 0" /><path d="M16 4.6a3.5 3.5 0 0 1 0 6.8M18.5 14.2A6.5 6.5 0 0 1 21.5 20" /></>,
  repeat: <><path d="M17 2.5 20.5 6 17 9.5" /><path d="M3.5 12V10a4 4 0 0 1 4-4h13M7 21.5 3.5 18 7 14.5" /><path d="M20.5 12v2a4 4 0 0 1-4 4h-13" /></>,
  clip: <path d="m20 11.5-8 8a5 5 0 0 1-7-7l8.5-8.5a3.3 3.3 0 0 1 4.7 4.7L9.7 17.2a1.7 1.7 0 0 1-2.4-2.4L15 7" />,
  // 아티스트 페이지 소식·팬 응원
  heart: <path d="M12 20s-7.5-4.6-9-9.3C2 7.5 4 4.5 7.2 4.5c2 0 3.5 1.1 4.8 2.9 1.3-1.8 2.8-2.9 4.8-2.9 3.2 0 5.2 3 4.2 6.2C19.5 15.4 12 20 12 20Z" />,
  'heart-fill': <path d="M12 20s-7.5-4.6-9-9.3C2 7.5 4 4.5 7.2 4.5c2 0 3.5 1.1 4.8 2.9 1.3-1.8 2.8-2.9 4.8-2.9 3.2 0 5.2 3 4.2 6.2C19.5 15.4 12 20 12 20Z" fill="currentColor" />,
  pin: <><path d="M9 3.5h6l-1 6 3.5 3.5h-11L10 9.5Z" /><path d="M12 13v7.5" /></>,
  megaphone: <><path d="M4 10v4a1 1 0 0 0 1 1h2l8 4.5V4.5L7 9H5a1 1 0 0 0-1 1Z" /><path d="M7 15v3.5a1.5 1.5 0 0 0 3 0v-2M18.5 9.5a3.5 3.5 0 0 1 0 5" /></>,
  trash: <><path d="M4 7h16M9.5 7V4.5h5V7M6.5 7l1 12.5a1.5 1.5 0 0 0 1.5 1.5h6a1.5 1.5 0 0 0 1.5-1.5L17.5 7" /><path d="M10 11v6M14 11v6" /></>,
  'eye-off': <><path d="M3 3l18 18" /><path d="M10.6 5.1A9.8 9.8 0 0 1 12 5c5.5 0 9 5.5 9 7 0 .7-.8 2.2-2.2 3.6M6.6 6.6C4.4 8 3 10.6 3 12c0 1.5 3.5 7 9 7 1.6 0 3-.4 4.3-1.1" /><path d="M9.9 9.9a3 3 0 0 0 4.2 4.2" /></>,
  eye: <><path d="M3 12c0-1.5 3.5-7 9-7s9 5.5 9 7-3.5 7-9 7-9-5.5-9-7Z" /><circle cx="12" cy="12" r="3" /></>,
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
