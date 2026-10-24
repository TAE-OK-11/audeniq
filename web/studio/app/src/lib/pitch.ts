// 플레이리스트 피칭 단계 — 발매 상세 피칭 카드와 홈 '플레이리스트 피칭' 카드가 같이 쓴다
/** 머리 상태 배지 — 짧게 */
export const PITCH_BADGE: Record<string, { tone: 'blue' | 'violet' | 'green' | 'gray'; label: string }> = {
  SUBMITTED: { tone: 'violet', label: '접수' }, IN_REVIEW: { tone: 'violet', label: '검토 중' },
  PITCHED: { tone: 'blue', label: '플랫폼 전달' }, SELECTED: { tone: 'green', label: '선정' },
  NOT_SELECTED: { tone: 'gray', label: '미선정' },
};
/** 보낸 피칭이 지나는 길 — 선정·미선정은 마지막 단계의 결과 */
export const PITCH_STEPS = ['접수', '담당자 검토', '플랫폼 전달', '선정 결과'];
export const PITCH_STEP: Record<string, number> = { SUBMITTED: 0, IN_REVIEW: 1, PITCHED: 2, SELECTED: 3, NOT_SELECTED: 3 };
export const PLATFORM_KO: Record<string, string> = {
  spotify: 'Spotify', apple: 'Apple Music', melon: '멜론', genie: '지니', bugs: '벅스', flo: 'FLO', vibe: 'VIBE', youtube: 'YouTube Music',
};
