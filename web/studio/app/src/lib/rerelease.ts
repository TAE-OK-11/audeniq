import { isrcValid, normalizeIsrc, upcValid } from './dsp';

export const RERELEASE_KINDS = { transfer: '유통사를 AUDENIQ로 이전', redistribute: '서비스가 종료된 음원 재발매', new_version: '새 녹음·리믹스·변경 버전 발매' } as const;
export const RERELEASE_AVAILABILITY = { live: '현재 서비스 중', takedown_requested: '이전 유통사에 서비스 종료 요청', removed: '서비스 종료됨', unknown: '확인이 필요해요' } as const;
export interface RereleaseData {
  rereleaseKind?: keyof typeof RERELEASE_KINDS | '';
  previousDistributor?: string; previousUrl?: string; previousUpc?: string;
  previousReleaseDate?: string;
  previousAvailability?: keyof typeof RERELEASE_AVAILABILITY | '';
  rereleaseAudio?: 'same' | 'changed' | 'unknown' | '';
  rereleaseRights?: 'owned' | 'permission' | 'pending' | '';
  rereleaseNotes?: string; rereleaseAck?: boolean;
  rereleasePermissionFile?: string; rereleasePermissionAssetId?: string;
  rereleaseTracks?: { trackId: string; previousIsrc: string }[];
}

export function rereleaseIssue(o: RereleaseData & { previousTitle: string }, tracks: { id: string; isrc: string }[], originalDate: string, releaseDate: string): { message: string; field: string } | null {
  const fail = (message: string, field: string) => ({ message, field });
  if (!o.rereleaseKind) return fail('이전·재발매하는 상황을 선택해 주세요.', 'aqRereleaseKind');
  if (!o.previousTitle.trim()) return fail('기존 발매명을 입력해 주세요.', 'aqPreviousTitle');
  if (!originalDate || originalDate > releaseDate) return fail('최초 발매일은 이번 발매일 이전 또는 같은 날로 입력해 주세요.', 'aqOriginalDate');
  if (!o.previousAvailability) return fail('기존 음원의 서비스 상태를 선택해 주세요.', 'aqPreviousAvailability');
  if (!o.rereleaseAudio || o.rereleaseAudio === 'unknown') return fail('이번 음원이 기존 녹음과 같은지 확인해 주세요. 확인 중에는 임시 저장할 수 있어요.', 'aqRereleaseAudio');
  if (o.rereleaseKind === 'new_version' && o.rereleaseAudio !== 'changed') return fail('같은 녹음이라면 유통사 이전 또는 서비스 종료된 음원 재발매를 선택해 주세요.', 'aqRereleaseKind');
  if (o.rereleaseKind !== 'new_version' && o.rereleaseAudio === 'changed') return fail('녹음·편곡을 변경했다면 새 녹음·변경 버전 발매를 선택해 주세요.', 'aqRereleaseKind');
  if (!o.rereleaseRights || o.rereleaseRights === 'pending') return fail('이전 계약과 이번 배급 권한을 확인한 후 접수해 주세요. 작성 내용은 임시 저장할 수 있어요.', 'aqRereleaseRights');
  if (o.previousAvailability === 'unknown' && !o.rereleaseNotes?.trim()) return fail('서비스 상태 확인이 필요한 내용을 적어 주세요.', 'aqRereleaseNotes');
  if (o.previousUpc && !upcValid(o.previousUpc)) return fail('기존 UPC 형식을 확인해 주세요.', 'aqPreviousUpc');
  if (o.previousUrl) {
    try { if (new URL(o.previousUrl).protocol !== 'https:') throw new Error(); }
    catch { return fail('기존 발매 링크는 https 주소로 입력해 주세요.', 'aqPreviousUrl'); }
  }
  for (const [i, t] of tracks.entries()) {
    const previous = o.rereleaseTracks?.find(x => x.trackId === t.id)?.previousIsrc ?? '';
    if (previous && !isrcValid(previous)) return fail(`트랙 ${i + 1}의 기존 ISRC 형식을 확인해 주세요.`, `aqPreviousIsrc-${i}`);
    if (o.rereleaseAudio === 'same' && (!previous || normalizeIsrc(previous) !== normalizeIsrc(t.isrc))) return fail(`트랙 ${i + 1}은 같은 녹음이에요. 기존 ISRC를 입력하고 트랙에 적용해 주세요.`, `aqPreviousIsrc-${i}`);
    if (o.rereleaseAudio === 'changed' && previous && normalizeIsrc(previous) === normalizeIsrc(t.isrc)) return fail(`트랙 ${i + 1}의 변경된 녹음에 기존 ISRC를 재사용할 수 없어요. 트랙 등록에서 새 코드로 바꾸거나 비워 주세요.`, `aqPreviousIsrc-${i}`);
  }
  if (!o.rereleaseAck) return fail('이전·재발매 진행 안내를 확인해 주세요.', 'aqRereleaseAck');
  return null;
}

export function rereleaseSummary(o: RereleaseData & { previousTitle?: string }): string {
  return [o.rereleaseKind && RERELEASE_KINDS[o.rereleaseKind], o.previousTitle,
    o.previousAvailability && RERELEASE_AVAILABILITY[o.previousAvailability],
    o.rereleaseAudio === 'same' ? '기존 녹음·ISRC 유지' : o.rereleaseAudio === 'changed' ? '변경된 녹음·새 ISRC' : '',
    o.previousDistributor && `이전 유통사: ${o.previousDistributor}`].filter(Boolean).join(' · ');
}
