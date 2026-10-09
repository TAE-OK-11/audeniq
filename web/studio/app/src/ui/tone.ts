/** 상태 색조 — 배지·숫자 칸·아이콘 칸·알림이 같은 짝(옅은 바탕 + 진한 글자)을 쓴다 */
export type Tone = 'blue' | 'violet' | 'amber' | 'green' | 'red' | 'gray';

/** 클래스 이름 합치기 (빈 값은 건너뜀) */
export const cx = (...parts: (string | false | null | undefined)[]) => parts.filter(Boolean).join(' ');
