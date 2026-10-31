// 배급 화면 곳곳에 붙는 퍼블리싱 조각 — 앨범 단위 설정은 발매 상세 '퍼블리싱' 탭에서.
//  · takePublishingIntent: 발매를 접수할 때 적은 크레딧으로 곡 정보를 미리 정리해 둘지 (기본: 정리)
//  · PublishingLinkRow: 발매 상세·곡 시트의 '작사·작곡 저작권' 한 줄 (앨범이면 발매 상세 '퍼블리싱' 탭으로)
//  · PublishingBundleStep: 공유하는 계약서 화면의 발매 계약 묶음 — 배급 계약 다음 줄에 같은 앨범의 저작권 관리 계약
import { Link, useNavigate } from '../../lib/router';
import { pubApi, type Work } from '../../api/publishing';
import { useAsync } from '../../hooks/useAsync';
import { usePublishingUse } from '../../hooks/usePublishingUse';
import { readJSON, removeKey } from '../../lib/storage';
import { Glyph } from '../Glyph';
import { IconTile } from '../../ui';
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

/** 배급 화면에 남기는 퍼블리싱 연결 한 줄 — 이 발매 곡들의 작사·작곡 저작권이 어디까지 왔는지 + 퍼블리싱에서 보기.
 *  자세한 곡별 상태·신청·계약은 퍼블리싱 메뉴에서만 다룬다 */
export function PublishingLinkRow({ releaseId, trackId }: { releaseId?: string; trackId?: string }) {
  const { data: works } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  if (!works) return null;
  const linked = works.filter(w => w.recordings.some(r => (!releaseId || r.releaseId === releaseId) && (!trackId || r.trackId === trackId)));
  const one = trackId ? linked[0] : undefined;
  const managed = linked.filter(w => w.registration === 'registered' || w.contractId).length;
  const meta = one ? (one.registration === 'registered' ? '저작권 등록 완료' : one.contractId ? '저작권 관리 중' : '저작권 관리 전')
    : !linked.length ? '작사·작곡 저작권을 맡기면 저작권료까지 받아요'
    : managed ? `${linked.length}곡 중 ${managed}곡 관리 중` : '아직 맡긴 곡이 없어요';
  return (
    <Link to={one ? `/works/${one.id}` : releaseId ? `/releases/${encodeURIComponent(releaseId)}?tab=publishing` : '/releases?tab=works'} className="ui-row aq-pub-link">
      <IconTile size="sm"><Glyph name="scale" size={16} /></IconTile>
      <span className="min-0">
        <span className="ui-row-title">{one || !releaseId ? '작사·작곡 저작권' : '이 앨범 퍼블리싱'}</span>
        <span className="ui-row-meta"><span>{meta}</span></span>
      </span>
      <Glyph name="chevron-right" size={16} />
    </Link>
  );
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
  const status = fix ? { tone: 'amber' as const, label: '보완 필요' }
    : managed.length && managed.length >= trackCount ? { tone: 'green' as const, label: '관리 중' }
    : applying.length ? { tone: 'violet' as const, label: '심사 중' }
    : managed.length ? { tone: 'blue' as const, label: `${managed.length}곡 관리 중` }
    : null;
  return { mine, managed, applying, fix, status };
}
