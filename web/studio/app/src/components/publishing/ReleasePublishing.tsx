// 배급 화면 곳곳에 붙는 작사·작곡 저작권 조각 — 앨범 단위 설정은 발매 상세 '배급·계약' 탭의 저작권 칸에서.
//  · takePublishingIntent: 발매를 접수할 때 적은 크레딧으로 곡 정보를 미리 정리해 둘지 (기본: 정리)
//  · PublishingBundleStep: 공유하는 계약서 화면의 발매 계약 묶음 — 배급 계약 다음 줄에 같은 앨범의 저작권 관리 계약
import { useNavigate } from '../../lib/router';
import { pubApi, type Work } from '../../api/publishing';
import { useAsync } from '../../hooks/useAsync';
import { usePublishingUse } from '../../hooks/usePublishingUse';
import { readJSON, removeKey } from '../../lib/storage';
import { CheckIcon } from '../Check';

export type PublishingIntent = 'works' | 'enroll' | 'none';
const intentKey = (draftId: string | null | undefined) => `publishing-intent:${draftId || 'new'}`;

/** 접수한 뒤 고른 내용을 꺼낸다 (새 발매는 접수 전 id가 없어 'new'에 둔다) */
export function takePublishingIntent(draftId: string | null | undefined): PublishingIntent {
  const v = readJSON<PublishingIntent | null>(intentKey(draftId), null) ?? readJSON<PublishingIntent | null>(intentKey(null), null);
  removeKey(intentKey(draftId));
  removeKey(intentKey(null));
  return v ?? 'works';
}

/** 계약 묶음의 세 번째 줄 — 배급 신청서 · 배급 계약서 다음에 같은 발매의 저작권 관리 계약 */
export function PublishingBundleStep({ releaseId }: { releaseId: string }) {
  const nav = useNavigate();
  const { data: works } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  // 퍼블리싱을 쓰지 않는 사람의 계약 묶음에는 저작권 줄을 붙이지 않는다 — 입구는 계약서 목록 아래 한 줄로
  const use = usePublishingUse();
  if (!works || !releaseId || !use.started) return null;
  const mine = works.filter(w => w.recordings.some(r => r.releaseId === releaseId) && w.myRole !== 'contributor');
  const managed = mine.filter(w => w.contractId);
  const applying = mine.filter(w => w.application && ['submitted', 'in_review', 'needs_fix'].includes(w.application.status));
  const fix = applying.find(w => w.application!.status === 'needs_fix');
  const done = managed.length > 0 && !applying.length;
  const state = done ? `${managed.length}곡 저작권 관리 중`
    : fix ? '저작권 관리 신청 보완 요청'
    : applying.length ? `${applying.length}곡 저작권 관리 심사 중`
    : '곡 저작권도 함께 맡길 수 있어요 (선택)';
  return (
    <li className={done ? 'is-done' : fix ? 'is-current' : ''}>
      <span className="aq-bundle-no" aria-hidden="true">{done ? <CheckIcon size={14} /> : 3}</span>
      <div className="min-0">
        <strong>저작권 관리 계약</strong>
        <span>{state}</span>
      </div>
      {done ? <button type="button" className="ui-btn is-soft is-sm" onClick={() => nav(`/contracts?pub=${managed[0].contractId}`)}>계약 보기</button>
        : applying.length ? <button type="button" className={fix ? 'ui-btn is-primary aq-bundle-sign' : 'ui-btn is-soft is-sm'} onClick={() => nav(`/works/${(fix ?? applying[0]).id}?tab=register`)}>{fix ? '보완하기' : '진행 보기'}</button>
        : <button type="button" className="ui-btn is-soft is-sm" onClick={() => nav(`/releases/${encodeURIComponent(releaseId)}?tab=publishing`)}>함께 신청</button>}
    </li>
  );
}

/** 앨범 단위 퍼블리싱 상태 — 발매 상세 '퍼블리싱' 탭과 퍼블리싱 메뉴가 같은 계산을 쓴다 */
export function albumPublishing(works: readonly Work[], releaseId: string, trackCount: number) {
  const mine = works.filter(w => w.recordings.some(r => r.releaseId === releaseId));
  const managed = mine.filter(w => w.registration === 'registered' || !!w.contractId);
  const applying = mine.filter(w => w.application && ['submitted', 'in_review', 'needs_fix'].includes(w.application.status));
  const fix = applying.some(w => w.application!.status === 'needs_fix') || mine.some(w => w.registration === 'needs_fix');
  // 트랙 기준으로 센다 — 한 곡이 원곡·Inst.처럼 두 트랙에 실리면 곡 수로는 '1곡 관리 중'이 됐다
  const tracksOf = (list: Work[]) => list.reduce((n, w) => n + w.recordings.filter(r => r.releaseId === releaseId).length, 0);
  const managedTracks = tracksOf(managed);
  const status = fix ? { tone: 'amber' as const, label: '보완 필요' }
    : managedTracks && managedTracks >= trackCount ? { tone: 'green' as const, label: '관리 중' }
    : applying.length ? { tone: 'violet' as const, label: '심사 중' }
    : managedTracks ? { tone: 'blue' as const, label: `${managedTracks}곡 관리 중` }
    : null;
  return { mine, managed, applying, fix, status };
}
