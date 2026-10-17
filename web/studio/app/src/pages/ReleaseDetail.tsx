import { useEffect, useState } from 'react';
import { Link, useNavigate, useParams } from '../lib/router';
import { api, type DeliveryItem } from '../api/client';
import { useToast } from '../components/Toast';
import { useConfirm } from '../components/Confirm';
import { TakedownCard } from '../components/TakedownCard';
import { CorrectionCard } from '../components/CorrectionCard';
import { PitchCard } from '../components/PitchCard';
import { SmartLinkCard } from '../components/SmartLinkCard';
import { SplitsCard } from '../components/SplitsCard';
import { ContentIdCard } from '../components/ContentIdCard';
import { TermCard } from '../components/TermCard';
import { InfringementCard } from '../components/InfringementCard';
import { SkeletonBlock, SkeletonRows } from '../components/Skeleton';
import { ReleaseCover, useImageOk, useSharpImage } from '../components/ReleaseCover';
import { coverImageUrl } from '../api/remote';
import { useAsync } from '../hooks/useAsync';
import { STATUS_LABEL, localStamp, statusTone } from '../lib/format';
import { dspLabel, durationLabel, genreLabel, kindLabel, languageLabel } from '../lib/catalog';
import { docBadgeTone, docState, docsForRelease, openFixes, pendingProofs, proofPath, useDocs } from '../store/docs';
import { refreshDocs } from '../store/portalSync';
import { MOCK } from '../lib/mode';
import { errorMessage } from '../api/errors';
import { CorrectionList, ReviewNote } from '../components/CorrectionList';
import { fixCountWord, fixPath, splitCorrections } from '../lib/corrections';
import { CheckIcon } from '../components/Check';
import { Glyph } from '../components/Glyph';
import { ContractBundle } from '../components/ContractBundle';

/** 아직 없는 값 — 실제 값처럼 진하게 보이지 않도록 흐리게 */
const Empty = ({ children }: { children: string }) => <span className="aq-empty-val">{children}</span>;
import { Badge, Tabs } from '../ui';
import type { Tone } from '../ui/tone';
import { territorySummary } from '../lib/territories';

// 플랫폼별 배급 진행 단계 (서버 delivery_staging 기준)
const DELIVERY_STAGE: Record<string, string> = {
  NEEDS_CORRECTION: '보완 필요',
  IN_REVIEW: '배급 검토 중',
  PREPARING: '플랫폼 연결 준비 중',
  SCHEDULED: '전송 대기',
  ON_HOLD: '보류',
  SENDING: '전송 중',
  DELIVERED: '전송 완료',
  LIVE: '공개됨',
  TAKEN_DOWN: '내려감',
};

// 플랫폼 기준으로 고쳐야 하는 항목 안내
const DELIVERY_ISSUE: Record<string, string> = {
  DSP_ARTWORK_NOT_SQUARE: '커버아트를 1:1 정사각형으로 바꿔 주세요.',
  DSP_ARTWORK_TOO_SMALL: '이 플랫폼 기준보다 커버아트 해상도가 낮아요.',
  DSP_ARTWORK_TOO_LARGE: '이 플랫폼이 받는 크기보다 커버아트가 커요. 해상도를 줄여 주세요.',
  DSP_AUDIO_NOT_LOSSLESS: '무손실 음원(WAV·FLAC·ALAC·AIFF·WavPack·TTA)이 필요해요.',
  DSP_AUDIO_SAMPLE_RATE_LOW: '음원 음질(샘플레이트)이 기준 44.1kHz보다 낮아요. 원본으로 다시 올려 주세요.',
  DSP_AUDIO_BIT_DEPTH_LOW: '음원 음질(비트)이 기준 16bit보다 낮아요. 원본으로 다시 올려 주세요.',
  DSP_CREDIT_COMPOSER_MISSING: '곡마다 작곡가 크레딧이 필요해요.',
  DSP_CREDIT_LYRICIST_MISSING: '가사가 있는 곡은 작사가 크레딧이 필요해요.',
  DSP_GENRE_MISSING: '장르를 선택해 주세요.',
  DSP_LEAD_TIME_SHORT: '발매일까지 여유가 짧아 공개가 늦어질 수 있어요.',
  DSP_ARTWORK_UNMEASURED: '커버아트 크기를 확인하지 못했어요.',
  DSP_AUDIO_UNMEASURED: '음원 규격을 확인하지 못했어요.',
};

const RIGHTS_KEYS = ['rightsMaster', 'rightsComposition', 'rightsArtwork', 'rightsConsent'];

/** 상세 탭 — 휴대폰 한 줄에 다 보이게 넷으로 묶는다.
 *  정보(발매 정보·커버·트랙) · 배급·계약(플랫폼·권리·계약서) · 관리(발매 뒤에 하는 일) · 기록 */
const DETAIL_TABS = [
  { value: 'overview', label: '정보' },
  { value: 'delivery', label: '배급·계약' },
  { value: 'manage', label: '관리' },
  { value: 'history', label: '기록' },
];

const DELIVERY_TONE: Record<string, Tone> = {
  NEEDS_CORRECTION: 'amber', ON_HOLD: 'amber', IN_REVIEW: 'violet', PREPARING: 'blue', SCHEDULED: 'blue', SENDING: 'blue',
  DELIVERED: 'green', LIVE: 'green', TAKEN_DOWN: 'gray',
};

// 진행 단계 표시 — 상태별로 어디까지 왔는지 보여준다
const PIPELINE = [
  { key: 'draft', label: '작성' },
  { key: 'review', label: '검토' },
  { key: 'scheduled', label: '배급 승인' },
  { key: 'live', label: '발매 완료' },
];
// 진행 카드 머리에 쓰는 지금 단계 안내
const STAGE_NOTE: Record<string, string> = {
  draft: '신청서를 끝까지 작성한 뒤 접수해 주세요.',
  ready: '신청을 받았어요. 곧 담당자가 검토를 시작해요.',
  review: '담당자가 신청서를 검토하고 있어요. 결과는 알림으로 알려 드려요.',
  needs: '아래 요청을 고쳐 다시 접수하면 검토가 이어져요.',
  rejected: '검토 결과 이 발매는 배급할 수 없어요.',
  scheduled: '배급이 승인됐어요. 발매일에 맞춰 플랫폼으로 보내요.',
  live: '발매가 끝났어요. 플랫폼에서 들을 수 있어요.',
};
const pipelineIndex = (s: string) =>
  s === 'draft' ? 0 : s === 'ready' || s === 'review' || s === 'needs' || s === 'rejected' ? 1 : s === 'scheduled' ? 2 : s === 'live' ? 3 : 0;

function rightsOk(checks: Record<string, boolean> | undefined, options?: { sample?: boolean; featured?: boolean; ai?: boolean; shared?: boolean; rerelease?: boolean }): boolean {
  if (!RIGHTS_KEYS.every(k => checks?.[k])) return false;
  if ((options?.sample || options?.featured) && !checks?.rightsSamples) return false;
  if (options?.ai && !checks?.rightsAi) return false;
  if (options?.shared && !checks?.rightsShared) return false;
  if (options?.rerelease && !checks?.rightsRerelease) return false;
  return true;
}

export function ReleaseDetail() {
  const { id = '' } = useParams();
  const nav = useNavigate();
  const toast = useToast();
  const confirm = useConfirm();
  const allDocs = useDocs();
  // 계약서·서류 상태는 담당자 처리로 바뀐다 — 발매를 열 때 새로 받는다
  useEffect(() => { if (!MOCK) void refreshDocs(); }, [id]);
  const [tab, setTab] = useState('overview');
  const [deleting, setDeleting] = useState(false);
  const [withdrawing, setWithdrawing] = useState(false);
  const { data: rel, error, loading, reload } = useAsync(() => api.getRelease(id), [id]);
  // 배급 탭을 열 때만 플랫폼별 진행을 불러온다 (준비 전이면 빈 목록)
  const delivery = useAsync<DeliveryItem[]>(
    () => (tab === 'delivery' ? api.getDelivery(id).catch(() => []) : Promise.resolve([])),
    [id, tab],
  );
  // 훅은 아래 조기 반환보다 먼저 — 불러오기 전후로 호출 순서가 바뀌면 화면 오류가 난다
  // 큰 커버는 상세용 미리보기(1080px)로 — 목록용(240px)을 크게 키우면 뭉개진다. 원본은 서버에만 두고 받지 않는다.
  // 상세용 미리보기가 없는 예전 발매는 목록용 그대로 (커버를 다시 올리면 생긴다)
  const coverSrc = useSharpImage(
    rel?.draft?.coverData || rel?.coverData,
    !MOCK && rel?.draft?.coverPreviewAssetId ? coverImageUrl(rel.draft.coverPreviewAssetId, undefined) : undefined,
  );
  const [coverOk, coverFailed] = useImageOk(coverSrc);

  if (loading && !rel) {
    // 불러오는 동안에도 실제 화면과 같은 뼈대 — '발매 목록' 링크는 데이터와 상관없으니 그대로 두고, 큰 카드 자리도 같은 높이로.
    // 예전엔 링크 없이 큰 회색 칸부터 그려, 다 불러오면 화면 전체가 40px쯤 한 번에 움직였다
    return (
      <div id="view-release" className="view" aria-busy="true">
        <div className="spaced-actions">
          <button type="button" className="ui-btn is-white is-sm aq-back-link" onClick={() => nav('/releases')}><Glyph name="arrow-left" size={15} className="aq-glyph-lead" />발매 목록</button>
        </div>
        <SkeletonBlock height={160} className="aq-detail-skel" />
        <SkeletonRows count={3} />
      </div>
    );
  }
  if (error || !rel) {
    return (
      <div className="ui-empty">
        <h2>발매 정보를 불러오지 못했어요.</h2>
        <p>{error || '발매를 찾을 수 없어요.'}</p>
        <div className="row-actions" style={{ justifyContent: 'center' }}>
          <button type="button" className="ui-btn is-soft" onClick={reload}>다시 시도</button>
          <button type="button" className="ui-btn is-primary" onClick={() => nav('/releases')}>발매 목록으로</button>
        </div>
      </div>
    );
  }

  const d = rel.draft;
  const isDraft = rel.status === 'draft';
  const needsFix = rel.status === 'needs';
  const rejected = rel.status === 'rejected';
  // 실서버는 작성 중·보완 요청 상태만 고칠 수 있다 (그 밖은 마법사를 끝까지 채워도 NOT_EDITABLE).
  // 접수된 뒤의 수정은 문의로 요청하고, 취소된 발매는 고칠 일이 없다.
  const editInWizard = MOCK ? rel.status !== 'live' : isDraft;
  const editLabel = isDraft ? '계속 작성' : rel.status === 'needs' ? '보완하기' : editInWizard ? '수정하기' : '수정 요청';
  const showEdit = rel.status !== 'closed' && rel.status !== 'cancelled' && !rejected && !needsFix;
  const docs = docsForRelease(allDocs, rel.id, rel.title);
  const stage = pipelineIndex(rel.status);
  const genre = d?.genre ? genreLabel(d.genre) : '';
  // 발매 완료 후 3일이 지나면 진행 막대는 더 볼 필요가 없어 숨긴다
  const settledLive = rel.status === 'live' && !!rel.release_date
    && Date.now() - new Date(`${rel.release_date}T00:00:00`).getTime() >= 3 * 86_400_000;
  const stageTone = rejected ? 'is-error' : needsFix ? 'is-warn' : '';
  const stageLabel = (i: number) => (i === 1 && rejected ? '거절' : i === 1 && needsFix ? '보완 필요' : PIPELINE[i].label);
  const allFixes = rel.corrections ?? [];
  // 담당자 전체 의견은 고칠 항목이 아니라 따로 보여 준다
  const { items: requested, note: reviewNote } = splitCorrections(allFixes);
  // 요청 서류를 이미 낸 ‘추가 서류 첨부’ 항목은 끝난 것으로 본다 (남은 일은 다시 접수)
  const fixes = openFixes(requested, allDocs);
  // 담당자가 요청한 서류 — 신청서 보완과 따로, 그 요청의 첨부·제출 화면으로 보낸다
  // 보완 항목(추가 서류 첨부)으로 요청한 서류는 항목 목록에서 안내하므로 빼고 보여 준다
  const linkedDocs = new Set(requested.map(c => c.documentId).filter(Boolean));
  const proofs = pendingProofs(allDocs, rel.id, rel.title).filter(doc => !linkedDocs.has(doc.id));
  const docsOnly = needsFix && rel.fixKind === 'documents';
  // 이미 발매된 뒤에도 '발매 예정일'이라 적혀 있었다
  const dateLabel = rel.status === 'live' || rel.status === 'closed' ? '발매일' : '발매 예정일';
  // 배지 건수는 아래에 보여 주는 고칠 칸과 요청 서류를 모두 센다 (2건이라면서 3개를 보여 주던 것)
  const fixTotal = fixes.length + proofs.length;

  // 접수한 신청 취소 — 한 달 3회까지 직접, 그 이상·배급 시작 후는 문의로
  const canWithdraw = ['review', 'needs', 'scheduled'].includes(rel.status);
  const askByInquiry = () => nav(`/inquiries?new=cancel&release=${encodeURIComponent(rel.id)}`);
  const handleWithdraw = async () => {
    let q;
    try { q = await api.withdrawQuota(); } catch { q = null; }
    if (q && q.remaining <= 0) {
      const go = await confirm({
        title: '이번 달 취소 횟수를 모두 썼어요',
        message: `발매 신청은 한 달에 ${q.limit}번까지 직접 취소할 수 있어요. 더 취소하려면 문의로 요청해 주세요. 담당자가 확인 후 처리해요.`,
        confirmLabel: '문의로 취소 요청',
      });
      if (go) askByInquiry();
      return;
    }
    const ok = await confirm({
      title: '발매 신청을 취소할까요?',
      message: `‘${rel.title}’ 신청을 취소하면 되돌릴 수 없고, 다시 내려면 새 발매로 신청해야 해요.${q ? ` 이번 달 남은 직접 취소: ${q.remaining}회 (최대 ${q.limit}회)` : ''}`,
      confirmLabel: '신청 취소',
      danger: true,
    });
    if (!ok) return;
    setWithdrawing(true);
    try {
      const left = await api.withdrawRelease(rel.id);
      toast(`발매 신청을 취소했어요. 이번 달 남은 직접 취소 ${left.remaining}회`, 'success');
      reload();
    } catch (e) {
      const code = (e as { code?: string }).code;
      if (code === 'WITHDRAW_LIMIT_REACHED' || code === 'RELEASE_ALREADY_DELIVERING') {
        const go = await confirm({ title: '문의로 취소를 요청해 주세요', message: errorMessage(e), confirmLabel: '문의로 취소 요청' });
        if (go) askByInquiry();
      } else {
        toast(errorMessage(e, '취소하지 못했어요. 잠시 후 다시 시도해 주세요.'), 'error');
      }
    } finally {
      setWithdrawing(false);
    }
  };

  const handleDelete = async () => {
    const ok = await confirm({
      title: '발매를 삭제할까요?',
      message: `‘${rel.title}’ 발매를 카탈로그에서 삭제해요. 이 작업은 되돌릴 수 없어요.`,
      confirmLabel: '삭제',
      danger: true,
    });
    if (!ok) return;
    setDeleting(true);
    try {
      await api.deleteRelease(rel.id);
      toast('발매를 삭제했어요.');
      nav('/releases', { replace: true });
    } catch (e) {
      toast(errorMessage(e, '삭제에 실패했어요. 다시 시도해 주세요.'), 'error');
      setDeleting(false);
    }
  };

  // 발매 카드 버튼 — 넓은 화면은 카드 오른쪽 안, 휴대폰은 카드 아래 한 줄 (같은 버튼을 두 자리에)
  const actionButtons = (
    <>
      {(d?.application || !isDraft) && (
        <button type="button" className="ui-btn is-soft" onClick={() => nav(`/releases/${encodeURIComponent(rel.id)}/application`)}>
          신청서 보기
        </button>
      )}
      {/* 보완하기는 아래 보완 카드 안에 */}
      {showEdit && (
        <button
          type="button" className={`ui-btn ${isDraft ? 'is-primary' : 'is-soft'}`}
          onClick={() => nav(editInWizard ? `/upload?edit=${encodeURIComponent(rel.id)}` : `/inquiries?new=edit&release=${encodeURIComponent(rel.id)}`)}
        >
          {editLabel}
        </button>
      )}
      {/* 되돌릴 수 없는 일은 주 버튼과 같은 무게로 두지 않는다 — 조용한 빨간 글자 버튼 (누르면 확인 창) */}
      {canWithdraw && (
        <button type="button" className="ui-btn is-text aq-danger-text aq-withdraw-btn" onClick={handleWithdraw} disabled={withdrawing}>
          {withdrawing ? '취소하는 중' : '신청 취소'}
        </button>
      )}
      {isDraft && (
        <button type="button" className="ui-btn is-text aq-danger-text" onClick={handleDelete} disabled={deleting}>
          {deleting ? '삭제 중' : '삭제'}
        </button>
      )}
    </>
  );

  return (
    <div id="view-release" className="view">
      <div className="spaced-actions">
        <button type="button" className="ui-btn is-white is-sm aq-back-link" onClick={() => nav('/releases')}><Glyph name="arrow-left" size={15} className="aq-glyph-lead" />발매 목록</button>
      </div>

      <div className="aq-detail-hero" aria-label="발매 정보">
        <ReleaseCover id={rel.id} src={coverSrc} className="cover aq-detail-cover" />
        <div className="aq-detail-info">
          <div className="aq-detail-top">
            <span className="eyebrow">{kindLabel(d?.type) || '발매'}</span>
            {/* 진행 단계 카드가 보이면 상태는 거기서 — 같은 말을 두 번 하지 않는다 */}
            {settledLive && <Badge tone={statusTone(rel.status)} className={`aq-status is-${rel.status}`}>{STATUS_LABEL[rel.status] || rel.status}</Badge>}
          </div>
          <h1>{rel.title || '제목 없는 발매'}</h1>
          <p className="aq-detail-artist">{rel.artist || '아티스트 없음'}</p>
          {/* 비어 있는 값은 아래 '발매 정보'에서 '입력 전'으로 보여 주므로 여기서는 있는 것만 */}
          {(rel.tracks.length > 0 || genre || rel.release_date) && (
            <div className="aq-detail-facts">
              {rel.tracks.length > 0 && <span>{rel.tracks.length}곡</span>}
              {genre && <span>{genre}</span>}
              {rel.release_date && <span>{rel.release_date}</span>}
            </div>
          )}
        </div>
        <div className="aq-detail-actions is-wide">{actionButtons}</div>
      </div>

      <div className="row-actions aq-detail-actions is-narrow">{actionButtons}</div>

      {!settledLive && (
        <section className={`aq-stage${stageTone ? ` ${stageTone}` : ''}`} aria-label="발매 진행 단계">
          <div className="aq-stage-head">
            <div className="min-0">
              <span className="aq-stage-kicker">진행 단계</span>
              <strong className="aq-stage-now">{stageLabel(stage)}</strong>
            </div>
            <span className="aq-stage-count" aria-label={`${PIPELINE.length}단계 중 ${stage + 1}단계`}>
              {stage + 1}<small>/ {PIPELINE.length}</small>
            </span>
          </div>
          {STAGE_NOTE[rel.status] && !needsFix && !rejected && <p className="aq-stage-note">{STAGE_NOTE[rel.status]}</p>}
          <ol className="aq-stage-track">
            {PIPELINE.map((p, i) => {
              const done = i < stage || (i === stage && rel.status === 'live');
              return (
                <li key={p.key} className={done ? 'is-done' : i === stage ? 'is-current' : ''} aria-current={i === stage ? 'step' : undefined}>
                  <span className="aq-stage-bar" aria-hidden="true" />
                  <span className="aq-stage-label">
                    {done && <CheckIcon size={10} />}
                    {stageLabel(i)}
                  </span>
                </li>
              );
            })}
          </ol>
        </section>
      )}

      {rejected && (
        <section className="aq-fix-panel is-rejected" aria-labelledby="aqRejectHead">
          <div className="aq-fix-top"><Badge tone="red">배급 거절</Badge></div>
          <h2 id="aqRejectHead" className="aq-fix-title">이 발매는 배급할 수 없어요</h2>
          <div className="aq-fix-body">
            <ReviewNote text={reviewNote || '담당자 검토 결과 이 발매는 배급할 수 없어요. 궁금한 점은 문의로 남겨 주세요.'} />
            <p className="aq-fix-foot">거절된 발매는 다시 접수할 수 없어요. 내용을 고쳐 새 발매로 신청해 주세요.</p>
          </div>
        </section>
      )}

      {needsFix && (
        <section className="aq-fix-panel" aria-labelledby="aqFixCardHead">
          <div className="aq-fix-top">
            <Badge tone="amber">{docsOnly ? (proofs.length ? `서류 제출 요청 ${proofs.length}건` : '서류 확인 필요') : fixTotal ? `보완 요청 ${fixTotal}건` : '보완 필요'}</Badge>
          </div>
          <h2 id="aqFixCardHead" className="aq-fix-title">
            {docsOnly ? '담당자가 요청한 서류를 제출해 주세요'
              // 배지는 고칠 칸 + 요청 서류를 함께 센다 — 제목도 둘 다 말해야 '3건'과 '두 가지'가 어긋나지 않는다
              : fixes.length && proofs.length ? `${fixCountWord(fixes.length)} 고치고 서류 ${proofs.length}건을 내면 바로 다시 검토해요`
              : fixes.length ? `${fixCountWord(fixes.length)} 고치면 바로 다시 검토해요`
                : requested.length ? '요청한 서류를 냈어요. 신청서를 다시 접수해 주세요'
                : reviewNote ? '검토팀 의견을 확인하고 신청서를 고쳐 주세요' : '알림과 권리 서류에서 요청 내용을 확인해 주세요'}
          </h2>
          <div className="aq-fix-body">
            {reviewNote && <ReviewNote text={reviewNote} />}
            {proofs.length > 0 && (
              <ul className="aq-linked-docs" aria-label="요청받은 서류">
                {proofs.map(doc => (
                  <li key={doc.id}>
                    <Link to={proofPath(doc)}>
                      <span className="min-0">{doc.title.replace(`${rel.title} · `, '')}{doc.reviewNote ? ` — ${doc.reviewNote}` : ''}</span>
                      <Badge tone={docBadgeTone(doc)}>{docState(doc)}</Badge>
                    </Link>
                  </li>
                ))}
              </ul>
            )}
            {docsOnly ? (
              <button type="button" className="ui-btn is-primary aq-fix-cta" onClick={() => nav(proofPath(proofs[0]))}>
                {proofs.length ? '서류 첨부하러 가기' : '권리·보완 서류 확인하기'}
              </button>
            ) : fixes.length > 0 ? (
              <CorrectionList
                releaseId={rel.id} corrections={fixes}
                trackIds={(d?.draftTracks ?? rel.tracks).map(t => t.id)}
                trackTitles={Object.fromEntries((d?.draftTracks ?? rel.tracks).map(t => [t.id, t.title]))}
              />
            ) : (
              <button type="button" className="ui-btn is-primary aq-fix-cta" onClick={() => nav(fixPath(rel.id))}>신청서 고치러 가기</button>
            )}
          </div>
        </section>
      )}

      <Tabs label="발매 상세 메뉴" value={tab} items={DETAIL_TABS} onChange={setTab} />

      <div id="releaseDetailBody" className="ui-tab-panel" key={tab}>
        {tab === 'overview' && (
          <div className="aq-detail-stack">
            <div className="split">
              <div className="aq-relinfo-card">
                <h2 className="subhead">발매 정보</h2>
                <dl className="ui-kv is-2">
                  <div><dt>아티스트</dt><dd>{rel.artist || <Empty>입력 전</Empty>}</dd></div>
                  <div><dt>발매 유형</dt><dd>{kindLabel(d?.type) || <Empty>입력 전</Empty>}</dd></div>
                  <div><dt>주요 언어</dt><dd>{languageLabel(d?.language) || <Empty>입력 전</Empty>}</dd></div>
                  <div><dt>{dateLabel}</dt><dd>{rel.release_date || <Empty>미정</Empty>}</dd></div>
                  <div><dt>UPC / EAN</dt><dd>{d?.upc || <Empty>등록 전</Empty>}</dd></div>
                  <div><dt>장르</dt><dd>{genre || <Empty>입력 전</Empty>}</dd></div>
                  <div><dt>레이블</dt><dd>{d?.label || <Empty>입력 전</Empty>}</dd></div>
                </dl>
                <h2 className="subhead">앨범 소개</h2>
                <p className="muted small break">{d?.notes || '등록된 소개가 없어요.'}</p>
              </div>
              <div className="studio-album-aside aq-cover-card">
                <h2 className="subhead">커버아트</h2>
                <div className={`aq-cover-stage${coverOk ? '' : ' is-empty'}`}>
                  {coverOk ? (
                    <>
                      {/* 커버 색이 카드 바탕으로 번지게 — 같은 이미지를 흐리게 깐다 */}
                      <img className="aq-cover-glow" src={coverSrc} alt="" aria-hidden="true" decoding="async" />
                      <img className="aq-detail-cover-large" src={coverSrc} alt={`${rel.title} 커버아트`} decoding="async" onError={coverFailed} />
                    </>
                  ) : (
                    <div className="aq-detail-cover-empty">
                      {/* 이미지가 없을 땐 위 머리 카드와 같은 발매별 색 커버 */}
                      <ReleaseCover id={rel.id} className="cover aq-detail-cover-fallback" />
                      <span>{d?.coverName ? '미리보기 없음' : '커버아트 등록 전'}</span>
                    </div>
                  )}
                </div>
                <dl className="aq-cover-meta">
                  {d?.coverName && <div><dt>파일</dt><dd className="break">{d.coverName}</dd></div>}
                  <div><dt>최근 수정</dt><dd>{rel.updated_at ? localStamp(rel.updated_at) : localStamp(rel.created_at)}</dd></div>
                </dl>
              </div>
            </div>
            <div className="aq-relinfo-card">
              <h2 className="subhead">트랙 {rel.tracks.length}곡</h2>
              {rel.tracks.length ? (
                <div className="aq-catalog-cards aq-stagger">
                  {rel.tracks.map((t, i) => (
                    <div key={t.id} className="aq-track-card">
                      <span className="aq-track-num" aria-hidden="true">{i + 1}</span>
                      <div className="min-0">
                        <span className="ui-row-title">
                          {t.title || '곡명 없음'}{t.version ? ` (${t.version})` : ''}
                          {t.explicit && <em className="aq-explicit" title="Explicit">E</em>}
                        </span>
                        {/* 크레딧 · 길이 / 식별자 · 파일 — 잘리지 않게 두 줄로 나눈다 */}
                        <span className="ui-row-meta aq-track-credit">
                          작곡 {t.composers || '없음'} · 작사 {t.lyricists || '없음'}
                          {t.duration_ms ? ` · ${durationLabel(t.duration_ms)}` : ''}
                        </span>
                        <span className="aq-track-facts">
                          <span className="ui-tag">ISRC {t.isrc || '등록 전'}</span>
                          <span className={`ui-tag aq-track-file${t.audioName ? '' : ' is-missing'}`}>
                            {t.audioName ? <><Glyph name="check" size={12} />{t.audioName}</> : '음원 파일 없음'}
                          </span>
                        </span>
                      </div>
                    </div>
                  ))}
                </div>
              ) : (
                <div className="empty-note">
                  등록된 트랙이 없어요.<br />
                  <button type="button" className="ui-btn is-white is-sm" onClick={() => nav(`/upload?edit=${encodeURIComponent(rel.id)}`)}>트랙 등록하기 <Glyph name="chevron-right" size={13} /></button>
                </div>
              )}
              {/* 제출 전 안내 — 접수가 끝난 발매(검토 중·발매 완료)에서는 할 수 있는 일이 없는 문장이었다 */}
              {(isDraft || needsFix) && (
                <div className="ui-callout" style={{ marginTop: 22 }}>
                  음원 원본의 재첨부가 필요한 경우가 있어요. 제출 전 파일을 다시 확인해 주세요.
                </div>
              )}
            </div>
          </div>
        )}

        {tab === 'delivery' && (
          <div className="aq-detail-stack">
            <section className="aq-relinfo-card">
              <h2 className="subhead">배급 설정</h2>
              <dl className="ui-kv is-2">
                <div><dt>배급 지역</dt><dd>{territorySummary(d?.territories)}</dd></div>
                <div><dt>{dateLabel}</dt><dd>{rel.release_date || <Empty>미정</Empty>}</dd></div>
                {d?.originalDate && <div><dt>최초 발매일</dt><dd>{d.originalDate}</dd></div>}
              </dl>
              <div className="tag-row">
                {d?.platforms?.length ? (
                  d.platforms.map(p => <span key={p} className="ui-tag is-white">{dspLabel(p)}</span>)
                ) : (
                  <span className="muted small">플랫폼 미선택</span>
                )}
              </div>
            </section>
            {!!delivery.data?.length && (
              <section className="aq-relinfo-card">
                <h2 className="subhead">플랫폼별 진행 상황</h2>
                <ul className="aq-linked-docs">
                  {delivery.data.map(item => (
                    <li key={item.dsp}>
                      <div className="min-0">
                        <span>{dspLabel(item.slug)}</span>
                        {item.issues.filter(i => i.severity === 'BLOCKER').map(i => (
                          <p key={i.code} className="small muted">{DELIVERY_ISSUE[i.code] ?? '이 플랫폼 기준에 맞게 확인이 필요해요.'}</p>
                        ))}
                      </div>
                      <Badge tone={DELIVERY_TONE[item.stage] ?? 'gray'}>{DELIVERY_STAGE[item.stage] ?? item.stage}</Badge>
                    </li>
                  ))}
                </ul>
              </section>
            )}
            <section className="aq-relinfo-card">
              <div className="ui-section-head">
                <h2 className="subhead">권리</h2>
                {rightsOk(d?.rightsChecks, d?.options)
                  ? <Badge tone="green">권리 확인 완료</Badge>
                  : <Badge tone="amber">권리 확인 보완 필요</Badge>}
              </div>
              <dl className="ui-kv is-2">
                <div><dt>마스터 권리자</dt><dd>{d?.ownership || <Empty>입력 전</Empty>}</dd></div>
                <div><dt>℗ 표기</dt><dd>{d?.phonogram || <Empty>입력 전</Empty>}</dd></div>
                <div><dt>© 표기</dt><dd>{d?.copyright || <Empty>입력 전</Empty>}</dd></div>
              </dl>
            </section>
            <section className="aq-relinfo-card">
              <h2 className="subhead">계약</h2>
              <ContractBundle
                compact releaseId={rel.id} releaseTitle={rel.title}
                agreement={docs.find(x => x.kind === 'agreements')}
                hasApplication={!!d?.application || !isDraft}
                onOpenAgreement={docId => nav(`/contracts?doc=${encodeURIComponent(docId)}`)}
              />
            </section>
            {docs.some(x => x.kind === 'rights') && (
              <section className="aq-relinfo-card">
                <h2 className="subhead">권리 서류</h2>
                <ul className="aq-linked-docs">
                  {docs.filter(x => x.kind === 'rights').map(doc => (
                    <li key={doc.id}>
                      <Link to={proofPath(doc)}>
                        <span className="min-0">{doc.title.replace(`${rel.title} · `, '')}</span>
                        <Badge tone={docBadgeTone(doc)}>{docState(doc)}</Badge>
                      </Link>
                    </li>
                  ))}
                </ul>
              </section>
            )}
          </div>
        )}

        {tab === 'manage' && (
          // 발매 뒤에 하는 일을 성격별로 묶는다 — 카드 8장이 한 줄로 쌓이면 무엇이 어디 있는지 안 보인다
          <div className="aq-detail-stack">
            {isDraft ? <SplitsCard releaseId={rel.id} /> : (
              <>
                <section className="aq-manage-group" aria-label="수익·홍보">
                  <h3 className="aq-manage-group-title">수익·홍보</h3>
                  <SplitsCard releaseId={rel.id} />
                  <SmartLinkCard releaseId={rel.id} />
                  <PitchCard releaseId={rel.id} />
                </section>
                <section className="aq-manage-group" aria-label="권리 보호">
                  <h3 className="aq-manage-group-title">권리 보호</h3>
                  <ContentIdCard releaseId={rel.id} />
                  <InfringementCard releaseId={rel.id} />
                </section>
                <section className="aq-manage-group" aria-label="서비스·계약">
                  <h3 className="aq-manage-group-title">서비스·계약</h3>
                  <CorrectionCard releaseId={rel.id} />
                  <TakedownCard releaseId={rel.id} />
                  {!rejected && rel.status !== 'cancelled' && rel.status !== 'closed' && <TermCard releaseId={rel.id} />}
                </section>
              </>
            )}
          </div>
        )}

        {tab === 'history' && (
          <div className="aq-relinfo-card">
            <h2 className="subhead">변경 기록</h2>
            {d?.history?.length ? (
              <ol className="ui-timeline is-latest-first">
                {d.history.slice().reverse().map((h, i) => (
                  <li key={`${h.time}-${i}`}>
                    <strong>{h.text}</strong>
                    <small>{localStamp(h.time)}</small>
                  </li>
                ))}
              </ol>
            ) : (
              <div className="empty-note">등록된 변경 기록이 없어요.</div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
