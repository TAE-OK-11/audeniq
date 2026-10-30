// 앨범 퍼블리싱 — 발매 상세 '관리' 탭의 '퍼블리싱' 선택 카드에서 들어온다. 이 앨범 곡들의 작사·작곡 저작권을 앨범 단위로.
// 곡별 상태 → 한 번에 신청 → 계약 → 저작권료. 계약서·정산·알림·문의는 배급과 같은 화면을 함께 쓴다 (여기서는 그쪽으로 잇기만)
import { useState } from 'react';
import { Link, useNavigate, useParams } from '../lib/router';
import { api } from '../api/client';
import { pubApi, type Work } from '../api/publishing';
import { useAsync } from '../hooks/useAsync';
import { APP_LABEL, APP_TONE, pct } from '../lib/publishing';
import { Glyph } from '../components/Glyph';
import { ReleaseCover } from '../components/ReleaseCover';
import { SkeletonRows } from '../components/Skeleton';
import { RegBadge, roleText } from '../components/publishing/parts';
import { RoyaltyTable } from '../components/publishing/Royalties';
import { PublishingBundleStep, albumPublishing } from '../components/publishing/ReleasePublishing';
import { Badge, Callout, IconTile } from '../ui';
import { StageCard } from '../components/StageCard';
import { useToast } from '../components/Toast';
import { errorMessage } from '../api/errors';
import '../styles/split/ReleaseDetail.css';

const STEPS = ['곡 정리', '신청', '심사', '계약', '등록'] as const;

export function AlbumPublishing() {
  const { id = '' } = useParams();
  const nav = useNavigate();
  const { data: rel, error } = useAsync(() => api.getRelease(id), [id], { key: `release:${id}` });
  const { data: works, reload } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  const { data: royalties } = useAsync(() => pubApi.listRoyalties(), [], { key: 'royalties:all' });
  const [busy, setBusy] = useState(false);
  const toast = useToast();

  if (error) return <div className="view"><div className="ui-empty"><h2>앨범을 열 수 없어요.</h2><p>삭제됐거나 볼 수 있는 권한이 없어요.</p></div></div>;
  if (!rel || !works) return <div className="view"><SkeletonRows count={3} /></div>;

  const a = albumPublishing(works, rel.id, rel.tracks.length);
  const byTrack = new Map<string, Work>(a.mine.flatMap(w => w.recordings.filter(r => r.releaseId === rel.id).map(r => [r.trackId, w] as const)));
  const missing = rel.tracks.filter(t => !byTrack.has(t.id)).length;
  const canApply = a.mine.some(w => w.myRole !== 'contributor' && (w.registration === 'none' || w.registration === 'preparing') && (!w.application || w.application.status === 'withdrawn'));
  const step = a.managed.length >= rel.tracks.length && rel.tracks.length ? 4
    : a.managed.length ? 3 : a.applying.length ? 2 : a.mine.length && !missing ? 1 : 0;
  const workIds = new Set(a.mine.map(w => w.id));
  const albumRoyalties = (royalties ?? []).filter(r => workIds.has(r.workId));
  // 보완할 곡이 하나면 안내에서 바로 그 곡으로
  const fixWork = a.mine.find(w => w.registration === 'needs_fix' || w.application?.status === 'needs_fix');
  const applyBtn = canApply && <Link to={`/publishing/apply?release=${encodeURIComponent(rel.id)}`} className="ui-btn is-primary">이 앨범 저작권 관리 신청</Link>;
  const prepare = async () => {
    setBusy(true);
    try {
      await pubApi.prepareWorksFromRelease(rel.id, false); reload();
      toast(`${missing}곡을 크레딧으로 정리했어요. 지분이 맞는지 확인해 주세요.`, 'success');
    } catch (err) { toast(errorMessage(err, '정리하지 못했어요. 다시 시도해 주세요.'), 'error'); } finally { setBusy(false); }
  };

  return (
    <div id="view-release" className="view aq-album-pub">
      <div className="spaced-actions">
        <button type="button" className="ui-btn is-white is-sm aq-back-link" onClick={() => nav(`/releases/${rel.id}?tab=manage`)}>
          <Glyph name="arrow-left" size={15} className="aq-glyph-lead" />발매 상세
        </button>
      </div>

      <div className="aq-detail-hero" aria-label="앨범 퍼블리싱">
        <ReleaseCover id={rel.id} src={rel.coverData} className="cover aq-detail-cover" />
        <div className="aq-detail-info">
          <div className="aq-detail-top">
            <span className="eyebrow">퍼블리싱</span>
            {a.status && <Badge tone={a.status.tone}>{a.status.label}</Badge>}
          </div>
          <h1>{rel.title}</h1>
          <p className="aq-detail-artist">{rel.artist}</p>
          <p className="aq-detail-meta">{rel.tracks.length}곡 · 작사·작곡 저작권</p>
        </div>
        {/* 버튼·진행 단계 자리는 발매 상세와 같다 — 넓은 화면은 오른쪽 위, 휴대폰은 진행 단계 아래 */}
        {applyBtn && <div className="aq-detail-actions is-wide">{applyBtn}</div>}
        <StageCard steps={STEPS} current={step} complete={step === STEPS.length - 1} tone={a.fix ? 'warn' : undefined} label="퍼블리싱 진행 단계" />
        {applyBtn && <div className="aq-detail-actions is-narrow">{applyBtn}</div>}
      </div>

      <div className="aq-detail-stack">
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

        {/* 맡긴 곡이 없으면 빈 저작권료 카드는 숨긴다 — 할 일(곡 정리·신청)만 보이게 */}
        {(a.managed.length > 0 || albumRoyalties.length > 0) && <div className="aq-relinfo-card">
          <h2 className="subhead">이 앨범 저작권료</h2>
          <RoyaltyTable rows={albumRoyalties} emptyText="관리 단체가 사용료를 나눠 주면 곡별로 보여 드려요. 확정된 금액은 정산 잔액에 함께 쌓여요." />
        </div>}
      </div>
    </div>
  );
}
