// AUDENIQ 체크 — 체크박스·선택 타일·완료 표시가 모두 같은 모양을 쓴다 (글자 ✓ 대신 이 SVG)
export function CheckIcon({ size = 14, className }: { size?: number; className?: string }) {
  return (
    <svg className={`aq-check-icon${className ? ` ${className}` : ''}`} viewBox="0 0 16 16" width={size} height={size} aria-hidden="true" focusable="false">
      <path d="M3.5 8.4 6.6 11.5 12.6 4.8" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}
