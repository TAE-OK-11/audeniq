// 배급 화면 곳곳에 붙는 퍼블리싱 조각 — 앨범 단위 설정은 발매 상세 '퍼블리싱' 탭에서.
//  · takePublishingIntent: 발매를 접수할 때 적은 크레딧으로 곡 정보를 미리 정리해 둘지 (기본: 정리)
//  · PublishingLinkRow: 발매 상세·곡 시트의 '작사·작곡 저작권' 한 줄 (앨범이면 발매 상세 '퍼블리싱' 탭으로)
//  · PublishingBundleStep: 공유하는 계약서 화면의 발매 계약 묶음 — 배급 계약 다음 줄에 같은 앨범의 저작권 관리 계약
import { Link, useNavigate } from '../../lib/router';
import { pubApi, type Work } from '../../api/publishing';
import { signing } from '../../lib/publishingPrep';
import { useAsync } from '../../hooks/useAsync';
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

/** 계약 묶음의 세 번째 줄 — 배급 신청서 · 배급 계약서 다음에 같은 발매의 저작권 관리 계약서.
 *  배급 계약서처럼 담당자가 신청을 승인해 계약서가 만들어진 뒤에만 보인다. 신청·보완은 곡 화면에서 끝내고,
 *  여기에는 '확인하고 서명' 또는 '계약서 보기'만 둔다 */
export function PublishingBundleStep({ releaseId }: { releaseId: string }) {
  const nav = useNavigate();
  const { data: contracts } = useAsync(() => pubApi.listContracts(), [], { key: 'pub-contracts' });
  if (!contracts || !releaseId) return null;
  const mine = contracts.filter(c => c.releaseIds?.includes(releaseId));
  if (!mine.length) return null;
  const toSign = mine.find(c => c.status === 'pending_sign');
  const c = toSign ?? mine[0];
  const songs = c.works.length > 1 ? `‘${c.works[0].title}’ 외 ${c.works.length - 1}곡` : `‘${c.works[0]?.title ?? '곡'}’`;
  return (
    <li className={toSign ? 'is-current' : 'is-done'}>
      <span className="aq-bundle-no" aria-hidden="true">{toSign ? 3 : <CheckIcon size={14} />}</span>
      <div className="min-0">
        <strong>저작권 관리 계약서</strong>
        <span>{toSign ? `${songs} · 담당자 승인 완료, 서명 필요` : `${songs} · 서명 완료`}</span>
      </div>
      <button type="button" className={toSign ? 'ui-btn is-primary aq-bundle-sign' : 'ui-btn is-soft is-sm'} onClick={() => nav(`/contracts/publishing/${encodeURIComponent(c.id)}`)}>
        {toSign ? '확인하고 서명' : '계약서 보기'}
      </button>
    </li>
  );
}

/** 앨범 단위 퍼블리싱 상태 — 발매 상세 '퍼블리싱' 탭과 퍼블리싱 메뉴가 같은 계산을 쓴다 */
export function albumPublishing(works: readonly Work[], releaseId: string, trackCount: number) {
  const mine = works.filter(w => w.recordings.some(r => r.releaseId === releaseId));
  const managed = mine.filter(w => w.registration === 'registered' || !!w.contractId);
  const toSign = mine.filter(signing);
  const applying = mine.filter(w => w.application && ['submitted', 'in_review', 'needs_fix'].includes(w.application.status));
  const fix = applying.some(w => w.application!.status === 'needs_fix') || mine.some(w => w.registration === 'needs_fix');
  // 트랙 기준으로 센다 — 한 곡이 원곡·Inst.처럼 두 트랙에 실리면 곡 수로는 '1곡 관리 중'이 됐다
  const tracksOf = (list: Work[]) => list.reduce((n, w) => n + w.recordings.filter(r => r.releaseId === releaseId).length, 0);
  const managedTracks = tracksOf(managed);
  const status = fix ? { tone: 'amber' as const, label: '보완 필요' }
    : toSign.length ? { tone: 'blue' as const, label: '계약서 서명' }
    : managedTracks && managedTracks >= trackCount ? { tone: 'green' as const, label: '관리 중' }
    : applying.length ? { tone: 'violet' as const, label: '심사 중' }
    : managedTracks ? { tone: 'blue' as const, label: `${managedTracks}곡 관리 중` }
    : null;
  return { mine, managed, applying, toSign, fix, status };
}
