// AUDENIQ 아이콘 (스튜디오 Glyph와 같은 모양) — 기기마다 다르게 그려지는 글자 기호 ✓ ↗ ← → ◷ 대신 쓰는 SVG
const P = {
  check: <path d="M5 12.5 9.5 17 19 7.5" />,
  'arrow-up-right': <path d="M7 17 17 7M9 7h8v8" />,
  'arrow-right': <path d="M5 12h14M13 6l6 6-6 6" />,
  'arrow-left': <path d="M19 12H5M11 6l-6 6 6 6" />,
  clock: <><circle cx="12" cy="12" r="9" /><path d="M12 7v5l3.2 2" /></>,
} as const;

export function Icon({ name, size = 14, className }: { name: keyof typeof P; size?: number; className?: string }) {
  return (
    <svg className={`icon${className ? ` ${className}` : ''}`} viewBox="0 0 24 24" width={size} height={size}
      fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" focusable="false">
      {P[name]}
    </svg>
  );
}
