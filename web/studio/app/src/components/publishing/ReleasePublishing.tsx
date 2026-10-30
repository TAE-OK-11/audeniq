// 배급과 퍼블리싱을 한 흐름으로 — 발매 화면 곳곳에 들어가는 '작사·작곡 저작권' 조각.
//  · ReleasePublishingChoice: 발매 신청서 '권리' 단계. 곡 크레딧으로 저작물을 만들어 둘지, 퍼블리싱도 함께 맡길지 고른다
//  · ReleaseWorksCard: 발매 상세 '개요'. 곡마다 연결된 작품과 저작권 등록 상태, 한 번에 신청
//  · PublishingBundleStep: 계약 묶음(배급 신청서 → 배급 계약서) 다음 줄. 같은 발매의 퍼블리싱 계약 상태
// 모양은 모두 그 화면에 이미 있는 부품(subhead · ui-check · ui-row · aq-bundle 줄)을 쓴다
import { useEffect, useState } from 'react';
import { Link, useNavigate } from '../../lib/router';
import { pubApi, type Work } from '../../api/publishing';
import type { ReleaseDetail } from '../../api/types';
import { useAsync } from '../../hooks/useAsync';
import { readJSON, removeKey, writeJSON } from '../../lib/storage';
import { APP_LABEL, APP_TONE, pct } from '../../lib/publishing';
import { CheckIcon } from '../Check';
import { Glyph } from '../Glyph';
import { Badge, IconTile } from '../../ui';
import { RegBadge, roleText } from './parts';

export type PublishingIntent = 'works' | 'enroll' | 'none';
const intentKey = (draftId: string | null | undefined) => `publishing-intent:${draftId || 'new'}`;

/** 접수한 뒤 고른 내용을 꺼낸다 (새 발매는 접수 전 id가 없어 'new'에 둔다) */
export function takePublishingIntent(draftId: string | null | undefined): PublishingIntent {
  const v = readJSON<PublishingIntent | null>(intentKey(draftId), null) ?? readJSON<PublishingIntent | null>(intentKey(null), null);
  removeKey(intentKey(draftId));
  removeKey(intentKey(null));
  return v ?? 'works';
}

interface ChoiceTrack { id: string; title: string; composers: string; lyricists: string; instrumental?: boolean }

/** 발매 신청서 '권리' 단계 — 배급 신청과 같은 화면에서 저작권까지 */
export function ReleasePublishingChoice({ tracks, coverIds, draftId }: { tracks: ChoiceTrack[]; coverIds: string[]; draftId: string | null | undefined }) {
  const [intent, setIntent] = useState<PublishingIntent>(() => readJSON<PublishingIntent | null>(intentKey(draftId), null) ?? 'works');
  useEffect(() => { writeJSON(intentKey(draftId), intent); }, [intent, draftId]);
  const { data: policy } = useAsync(() => pubApi.policy(), [], { key: 'pub-policy' });
  const { data: works } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  const linked = new Map((works ?? []).flatMap(w => w.recordings.map(r => [r.trackId, w] as const)));
  const OPTIONS: [PublishingIntent, string, string][] = [
    ['works', '작사·작곡 정보로 저작물도 정리해 둘게요', '곡마다 작품이 만들어지고 녹음과 연결돼요. 지분은 나중에 확인해요.'],
    ['enroll', '저작권 관리(퍼블리싱)도 함께 맡길게요', `방송·스트리밍 저작권료까지 AUDENIQ이 받아 드려요${policy ? ` · 수수료 ${policy.commissionRate}%` : ''}. 접수 뒤 지분만 확인하면 바로 신청돼요.`],
    ['none', '지금은 안 할게요', '배급은 그대로 진행돼요. 발매 상세에서 언제든 다시 할 수 있어요.'],
  ];
  return (
    <>
      <h2 className="subhead">작사·작곡 저작권</h2>
      <p className="ui-field-help">음원(녹음)의 권리와 별개로, 곡을 쓴 사람에게는 작사·작곡 저작권이 있어요. 위에 적은 크레딧으로 바로 정리할 수 있어요.</p>
      <div className="ui-list is-inset">
        {tracks.filter(t => t.title.trim()).map(t => {
          const w = linked.get(t.id);
          const cover = coverIds.includes(t.id);
          const credits = [t.composers && `작곡 ${t.composers}`, !t.instrumental && t.lyricists && `작사 ${t.lyricists}`].filter(Boolean);
          return (
            <div key={t.id} className="ui-row is-flat">
              <span className="min-0">
                <span className="ui-row-title">{t.title}</span>
                <span className="ui-row-meta">{credits.length ? credits.map(c => <span key={String(c)}>{c}</span>) : <span>크레딧 입력 전</span>}</span>
              </span>
              {w ? <RegBadge status={w.registration} /> : cover ? <Badge tone="gray">커버곡 · 원곡 권리</Badge> : <Badge tone="blue">작품으로 정리</Badge>}
            </div>
          );
        })}
      </div>
      <div className="field-group" role="radiogroup" aria-label="작사·작곡 저작권">
        {OPTIONS.map(([v, title, sub]) => (
          <label key={v} className="ui-check is-plain">
            <input type="radio" name="publishingIntent" checked={intent === v} onChange={() => setIntent(v)} />
            <span><strong>{title}</strong><br /><small className="muted">{sub}</small></span>
          </label>
        ))}
      </div>
    </>
  );
}

/** 발매 상세 '개요' — 이 발매 곡들의 작품·저작권 (발매 정보 카드와 같은 aq-relinfo-card) */
export function ReleaseWorksCard({ rel }: { rel: ReleaseDetail }) {
  const nav = useNavigate();
  const { data: works, reload } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  const [busy, setBusy] = useState(false);
  if (!rel.tracks.length || !works) return null;
  const byTrack = new Map<string, Work>(works.flatMap(w => w.recordings.filter(r => r.releaseId === rel.id).map(r => [r.trackId, w] as const)));
  const missing = rel.tracks.filter(t => !byTrack.has(t.id)).length;
  const canApply = [...byTrack.values()].some(w => w.myRole !== 'contributor' && (w.registration === 'none' || w.registration === 'preparing') && (!w.application || w.application.status === 'withdrawn'));
  const prepare = async () => {
    setBusy(true);
    try { await pubApi.prepareWorksFromRelease(rel.id, false); reload(); } finally { setBusy(false); }
  };
  return (
    <div className="aq-relinfo-card">
      <div className="ui-section-head">
        <h2 className="subhead">작사·작곡 저작권</h2>
        {canApply && <button type="button" className="ui-btn is-soft is-sm" onClick={() => nav(`/publishing/apply?release=${encodeURIComponent(rel.id)}`)}>퍼블리싱 함께 신청</button>}
      </div>
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
                <span className="ui-row-meta"><span>작품과 연결 전</span></span>
              </span>
              <Glyph name="chevron-right" size={16} />
            </Link>
          );
        })}
      </div>
      {missing > 1 && (
        <div className="row-actions">
          <button type="button" className={`ui-btn is-text is-sm${busy ? ' is-busy' : ''}`} disabled={busy} onClick={() => void prepare()}>
            {busy ? '만드는 중' : `크레딧으로 ${missing}곡 한 번에 작품 만들기`}
          </button>
        </div>
      )}
    </div>
  );
}

/** 계약 묶음의 세 번째 줄 — 배급 신청서 · 배급 계약서 다음에 같은 발매의 퍼블리싱 계약 */
export function PublishingBundleStep({ releaseId }: { releaseId: string }) {
  const nav = useNavigate();
  const { data: works } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  if (!works || !releaseId) return null;
  const mine = works.filter(w => w.recordings.some(r => r.releaseId === releaseId) && w.myRole !== 'contributor');
  const managed = mine.filter(w => w.contractId);
  const applying = mine.filter(w => w.application && ['submitted', 'in_review', 'needs_fix'].includes(w.application.status));
  const fix = applying.find(w => w.application!.status === 'needs_fix');
  const done = managed.length > 0 && !applying.length;
  const state = done ? `${managed.length}곡 저작권 관리 중`
    : fix ? '퍼블리싱 신청 보완 요청'
    : applying.length ? `${applying.length}곡 퍼블리싱 심사 중`
    : '곡 저작권도 함께 맡길 수 있어요 (선택)';
  return (
    <li className={done ? 'is-done' : fix ? 'is-current' : ''}>
      <span className="aq-bundle-no" aria-hidden="true">{done ? <CheckIcon size={14} /> : 3}</span>
      <div className="min-0">
        <strong>퍼블리싱 계약</strong>
        <span>{state}</span>
      </div>
      {done ? <button type="button" className="ui-btn is-soft is-sm" onClick={() => nav(`/contracts?pub=${managed[0].contractId}`)}>계약 보기</button>
        : applying.length ? <button type="button" className={fix ? 'ui-btn is-primary aq-bundle-sign' : 'ui-btn is-soft is-sm'} onClick={() => nav(`/works/${(fix ?? applying[0]).id}?tab=register`)}>{fix ? '보완하기' : '진행 보기'}</button>
        : <button type="button" className="ui-btn is-soft is-sm" onClick={() => nav(`/publishing/apply?release=${encodeURIComponent(releaseId)}`)}>함께 신청</button>}
    </li>
  );
}
