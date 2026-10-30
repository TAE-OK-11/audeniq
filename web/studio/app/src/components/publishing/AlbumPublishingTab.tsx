// 발매 상세 '퍼블리싱' 탭 — 이 앨범 곡들의 작사·작곡 저작권을 '관리'하는 곳: 신청 · 곡 정리 · 계약.
// 번 돈은 한 장으로만 (받은 금액 · 확정 전 예상) — 월별 추이·곡별·자세한 내역은 퍼블리싱 메뉴에서 이 앨범을 골라 본다.
// 계약서·정산·알림·문의는 배급과 같은 화면을 함께 쓴다
// 탭을 열 때 받는다: 처음 여는 '정보' 탭에는 필요 없다
import { useState } from 'react';
import { Link } from '../../lib/router';
import type { ReleaseDetail } from '../../api/types';
import { pubApi, type Work } from '../../api/publishing';
import { errorMessage } from '../../api/errors';
import { useAsync } from '../../hooks/useAsync';
import { APP_LABEL, APP_TONE, pct } from '../../lib/publishing';
import { Glyph } from '../Glyph';
import { SkeletonBlock } from '../Skeleton';
import { StageCard } from '../StageCard';
import { ManageHead } from '../ManageHead';
import { useToast } from '../Toast';
import { RegBadge, roleText } from './parts';
import { royaltyTotals } from './Royalties';
import { Money } from '../Money';
import { PublishingBundleStep, albumPublishing } from './ReleasePublishing';
import { Badge, Callout, IconTile, KeyValue } from '../../ui';

const STEPS = ['곡 정리', '신청', '심사', '계약', '등록'] as const;

export function AlbumPublishingTab({ rel }: { rel: ReleaseDetail }) {
  const toast = useToast();
  const { data: works, reload } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  const { data: royalties } = useAsync(() => pubApi.listRoyalties(), [], { key: 'royalties:all' });
  const [busy, setBusy] = useState(false);
  if (!works) return <SkeletonBlock height={240} />;

  const a = albumPublishing(works, rel.id, rel.tracks.length);
  const byTrack = new Map<string, Work>(a.mine.flatMap(w => w.recordings.filter(r => r.releaseId === rel.id).map(r => [r.trackId, w] as const)));
  const missing = rel.tracks.filter(t => !byTrack.has(t.id)).length;
  const canApply = a.mine.some(w => w.myRole !== 'contributor' && (w.registration === 'none' || w.registration === 'preparing') && (!w.application || w.application.status === 'withdrawn'));
  const step = a.managed.length >= rel.tracks.length && rel.tracks.length ? 4
    : a.managed.length ? 3 : a.applying.length ? 2 : a.mine.length && !missing ? 1 : 0;
  const workIds = new Set(a.mine.map(w => w.id));
  const albumRoyalties = (royalties ?? []).filter(r => workIds.has(r.workId));
  const totals = royaltyTotals(albumRoyalties);
  // 보완할 곡이 있으면 안내에서 바로 그 곡으로
  const fixWork = a.mine.find(w => w.registration === 'needs_fix' || w.application?.status === 'needs_fix');
  const text = a.fix ? '심사 중에 보완할 곡이 생겼어요. 고쳐서 다시 내면 심사가 이어져요.'
    : a.managed.length ? `${rel.tracks.length}곡 중 ${a.managed.length}곡의 저작권을 AUDENIQ이 관리하고 있어요. 저작권료는 정산 잔액에 함께 쌓여요.`
    : a.applying.length ? `${a.applying.length}곡을 심사하고 있어요. 끝나면 알림으로 알려 드려요.`
    : '이 앨범 곡의 작사·작곡 저작권도 맡기면 방송·스트리밍 저작권료까지 받아 드려요. (선택)';
  const prepare = async () => {
    setBusy(true);
    try {
      await pubApi.prepareWorksFromRelease(rel.id, false); reload();
      toast(`${missing}곡을 크레딧으로 정리했어요. 지분이 맞는지 확인해 주세요.`, 'success');
    } catch (err) { toast(errorMessage(err, '정리하지 못했어요. 다시 시도해 주세요.'), 'error'); } finally { setBusy(false); }
  };

  return (
    <div className="aq-detail-stack aq-album-pub">
      {/* '관리' 탭 카드와 같은 머리 — 아이콘 · 제목 · 상태, 아래에 진행 단계와 할 일 하나 */}
      <section className="ui-card aq-manage-card" aria-labelledby="albumPubHead">
        <ManageHead id="albumPubHead" icon="scale" title="퍼블리싱" status={a.status} />
        <p className="ui-field-help">{text}</p>
        <StageCard steps={STEPS} current={step} complete={step === STEPS.length - 1} tone={a.fix ? 'warn' : undefined} label="퍼블리싱 진행 단계" />
        {canApply && (
          <div className="row-actions">
            <Link to={`/publishing/apply?release=${encodeURIComponent(rel.id)}`} className="ui-btn is-primary is-sm">이 앨범 저작권 관리 신청</Link>
          </div>
        )}
      </section>

      {a.fix && (
        <Callout tone="warn" action={fixWork && <Link to={`/works/${fixWork.id}`} className="ui-btn is-sm is-white">보완하기</Link>}>
          {fixWork ? `‘${fixWork.title}’에 보완 요청이 왔어요.` : '보완 요청이 왔어요. 아래 곡을 눌러 고쳐 주세요.'}
        </Callout>
      )}

      <div className="aq-relinfo-card">
        <div className="ui-section-head"><h2 className="subhead">곡별 저작권</h2></div>
        <div className="ui-list is-inset">
          {rel.tracks.map((t, i) => {
            const w = byTrack.get(t.id);
            return w ? (
              <Link key={t.id} to={`/works/${w.id}`} className="ui-row">
                <IconTile size="sm"><span className="aq-preview-no">{i + 1}</span></IconTile>
                <span className="min-0">
                  <span className="ui-row-title">{t.title}</span>
                  <span className="ui-row-meta">{w.contributors.map(c => <span key={c.id}>{c.name} {roleText(c)} {pct(c.share)}</span>)}</span>
                </span>
                {w.application && !['approved', 'withdrawn'].includes(w.application.status)
                  ? <Badge tone={APP_TONE[w.application.status]}>{APP_LABEL[w.application.status]}</Badge> : <RegBadge status={w.registration} />}
              </Link>
            ) : (
              <Link key={t.id} to={`/works/new?release=${encodeURIComponent(rel.id)}&track=${encodeURIComponent(t.id)}`} className="ui-row">
                <IconTile size="sm" tone="gray"><span className="aq-preview-no">{i + 1}</span></IconTile>
                <span className="min-0">
                  <span className="ui-row-title">{t.title || '곡명 없음'}</span>
                  <span className="ui-row-meta"><span>작사·작곡 정보 정리 전</span></span>
                </span>
                <Glyph name="chevron-right" size={16} />
              </Link>
            );
          })}
        </div>
        {missing > 0 && (
          <div className="row-actions">
            <button type="button" className={`ui-btn is-soft is-sm${busy ? ' is-busy' : ''}`} disabled={busy} onClick={() => void prepare()}>
              {busy ? '정리하는 중' : `크레딧으로 ${missing}곡 한 번에 정리`}
            </button>
          </div>
        )}
      </div>

      {/* 계약서는 배급과 같은 계약서 화면에 — 여기서는 이 앨범의 저작권 관리 계약 줄만 */}
      {(a.managed.length > 0 || a.applying.length > 0) && (
        <div className="aq-relinfo-card">
          <h2 className="subhead">계약</h2>
          <ol className="aq-bundle-docs"><PublishingBundleStep releaseId={rel.id} /></ol>
        </div>
      )}

      {/* 번 돈은 간단히 — 받은 금액과 확정 전 예상만. 추이·곡별·내역은 퍼블리싱 메뉴에서 이 앨범을 골라 둔 채로 */}
      {(a.managed.length > 0 || albumRoyalties.length > 0) && (
        <section className="aq-relinfo-card aq-album-earn" aria-labelledby="albumEarn">
          <div className="ui-section-head">
            <h2 id="albumEarn" className="subhead">이 앨범 저작권료</h2>
            {albumRoyalties.length > 0 && <Link to={`/publishing?release=${encodeURIComponent(rel.id)}`} className="aq-head-link">자세히 보기<Glyph name="chevron-right" size={14} /></Link>}
          </div>
          {albumRoyalties.length ? (
            <KeyValue cols={2} items={[
              ['지금까지 받은 저작권료', <b className="aq-pub-amount"><Money value={totals.confirmed + totals.paid} animate /></b>],
              ['확정 전 예상', totals.estimated ? <Money value={totals.estimated} /> : '—'],
            ]} />
          ) : (
            <p className="ui-field-help">관리 단체가 사용료를 나눠 주면 여기에 보여 드려요. 보통 사용된 달부터 3~6개월 걸려요.</p>
          )}
        </section>
      )}
    </div>
  );
}
