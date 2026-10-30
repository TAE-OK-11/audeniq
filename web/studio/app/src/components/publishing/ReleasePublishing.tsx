// 배급 쪽에 남는 퍼블리싱 조각 — 곡의 작사·작곡 저작권은 퍼블리싱 메뉴에서만 다루고, 배급 화면에는 연결 한 줄만 둔다.
//  · takePublishingIntent: 발매를 접수할 때 적은 크레딧으로 곡 정보를 미리 정리해 둘지 (기본: 정리)
//  · PublishingLinkRow: 발매 상세·곡 시트·계약서 화면의 '작사·작곡 저작권은 퍼블리싱에서' 한 줄
import { Link } from '../../lib/router';
import { pubApi } from '../../api/publishing';
import { useAsync } from '../../hooks/useAsync';
import { readJSON, removeKey } from '../../lib/storage';
import { Glyph } from '../Glyph';
import { IconTile } from '../../ui';

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
    <Link to={one ? `/works/${one.id}` : '/publishing?tab=songs'} className="ui-row aq-pub-link">
      <IconTile size="sm"><Glyph name="scale" size={16} /></IconTile>
      <span className="min-0">
        <span className="ui-row-title">작사·작곡 저작권은 퍼블리싱에서</span>
        <span className="ui-row-meta"><span>{meta}</span></span>
      </span>
      <Glyph name="chevron-right" size={16} />
    </Link>
  );
}
