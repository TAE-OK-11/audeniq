// 플랫폼 사전 공개 (신청서의 '사전 공개 시작일' → DDEX ReleaseDisplayStartDate·예약 판매 딜).
// 이 날부터 스트리밍 플랫폼에 발매 예정 페이지가 열려 팬이 그 앱에서 미리 저장하고, 다운로드 스토어는 예약 판매를 받는다.
import { daysBetween } from './home';
import { shiftDate, todayStr } from './date';

/** 플랫폼별로 팬이 하는 것 — 표시 시점·이름은 플랫폼마다 조금 다르다 */
export const PRE_RELEASE_PLATFORMS: { id: string; name: string; what: string }[] = [
  { id: 'spotify', name: 'Spotify', what: '발매 예정 페이지에서 미리 저장 · Spotify for Artists에서 카운트다운 페이지' },
  { id: 'apple', name: 'Apple Music', what: '발매 예정 앨범 미리 추가(Pre-Add) · iTunes 예약 구매' },
  { id: 'amazon', name: 'Amazon Music', what: '발매 예정 페이지 · 다운로드 예약 구매' },
  { id: 'deezer', name: 'Deezer', what: '발매 예정 페이지에서 미리 저장' },
];

export type PreReleasePhase = 'none' | 'scheduled' | 'open' | 'released';

export interface PreReleaseState { phase: PreReleasePhase; daysToOpen: number; daysToRelease: number }

export function preReleaseState(preorderStart: string | null | undefined, releaseDate: string | null | undefined, today = todayStr()): PreReleaseState {
  if (!releaseDate) return { phase: 'none', daysToOpen: 0, daysToRelease: 0 };
  const daysToRelease = daysBetween(today, releaseDate);
  if (daysToRelease <= 0) return { phase: 'released', daysToOpen: 0, daysToRelease };
  if (!preorderStart) return { phase: 'none', daysToOpen: 0, daysToRelease };
  const daysToOpen = daysBetween(today, preorderStart);
  return { phase: daysToOpen > 0 ? 'scheduled' : 'open', daysToOpen, daysToRelease };
}

/** 신청서 빠른 선택 — 발매 1·2·4주 전 중 아직 지나지 않았고 90일 안인 날 */
export function preorderChoices(releaseDate: string, today = todayStr()): { weeks: number; date: string }[] {
  if (!releaseDate) return [];
  return [1, 2, 4]
    .map(weeks => ({ weeks, date: shiftDate(releaseDate, -7 * weeks) }))
    .filter(c => c.date >= today);
}

/** 발매 전 스마트링크 버튼 — 그 앱의 발매 예정 페이지에서 하는 일 */
export function preReleaseAction(platform: string): string | null {
  switch (platform) {
    case 'spotify':
    case 'deezer':
      return '미리 저장';
    case 'apple':
      return '미리 추가';
    case 'amazon':
      return '미리 보기';
    default:
      return null;
  }
}
