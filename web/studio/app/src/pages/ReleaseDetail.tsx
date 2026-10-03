import { useState } from 'react';
import { Link, useNavigate, useParams } from '../lib/router';
import { api, type DeliveryItem } from '../api/client';
import { useToast } from '../components/Toast';
import { useConfirm } from '../components/Confirm';
import { SkeletonBlock, SkeletonRows } from '../components/Skeleton';
import { ReleaseCover } from '../components/ReleaseCover';
import { useAsync } from '../hooks/useAsync';
import { STATUS_LABEL, localStamp } from '../lib/format';
import { dspLabel, durationLabel, genreLabel, kindLabel, languageLabel } from '../lib/catalog';
import { docState, docsForRelease, useDocs } from '../store/docs';
import { errorMessage } from '../api/errors';
import { CorrectionList, ReviewNote } from '../components/CorrectionList';
import { fixPath, splitCorrections } from '../lib/corrections';
import { CheckIcon } from '../components/Check';
import { Glyph } from '../components/Glyph';
import { ContractBundle } from '../components/ContractBundle';

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

const DETAIL_TABS = [
  { value: 'overview', label: '기본 정보' },
  { value: 'tracks', label: '트랙·파일' },
  { value: 'delivery', label: '배급·권리' },
  { value: 'history', label: '변경 기록' },
];

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
  ready: '접수됐어요. 곧 담당자가 검토를 시작해요.',
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
  const [tab, setTab] = useState('overview');
  const [deleting, setDeleting] = useState(false);
  const [withdrawing, setWithdrawing] = useState(false);
  const { data: rel, error, loading, reload } = useAsync(() => api.getRelease(id), [id]);
  // 배급 탭을 열 때만 플랫폼별 진행을 불러온다 (준비 전이면 빈 목록)
  const delivery = useAsync<DeliveryItem[]>(
    () => (tab === 'delivery' ? api.getDelivery(id).catch(() => []) : Promise.resolve([])),
    [id, tab],
  );

  if (loading && !rel) {
    return (
      <div className="view" aria-busy="true">
        <SkeletonBlock height={190} />
        <div style={{ height: 24 }} />
        <SkeletonRows count={3} />
      </div>
    );
  }
  if (error || !rel) {
    return (
      <div className="empty-page">
        <h2>발매 정보를 불러오지 못했어요.</h2>
        <p>{error || '발매를 찾을 수 없어요.'}</p>
        <div className="row-actions" style={{ justifyContent: 'center' }}>
          <button type="button" className="button secondary" onClick={reload}>다시 시도</button>
          <button type="button" className="button" onClick={() => nav('/releases')}>발매 목록으로</button>
        </div>
      </div>
    );
  }

  const d = rel.draft;
  const isDraft = rel.status === 'draft';
  const editLabel = isDraft ? '계속 작성' : rel.status === 'needs' ? '보완하기' : rel.status === 'live' ? '수정 요청' : '수정하기';
  const docs = docsForRelease(allDocs, rel.id, rel.title);
  const stage = pipelineIndex(rel.status);
  const genre = d?.genre ? genreLabel(d.genre) : '';
  const needsFix = rel.status === 'needs';
  const rejected = rel.status === 'rejected';
  // 발매 완료 후 3일이 지나면 진행 막대는 더 볼 필요가 없어 숨긴다
  const settledLive = rel.status === 'live' && !!rel.release_date
    && Date.now() - new Date(`${rel.release_date}T00:00:00`).getTime() >= 3 * 86_400_000;
  const stageTone = rejected ? 'is-error' : needsFix ? 'is-warn' : '';
  const stageLabel = (i: number) => (i === 1 && rejected ? '거절' : i === 1 && needsFix ? '보완 필요' : PIPELINE[i].label);
  const coverSrc = d?.coverData || rel.coverData;
  const allFixes = rel.corrections ?? [];
  // 담당자 전체 의견은 고칠 항목이 아니라 따로 보여 준다
  const { items: fixes, note: reviewNote } = splitCorrections(allFixes);

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
        toast(errorMessage(e, '취소하지 못했어요. 잠시 후 다시 시도해 주세요.'));
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
      toast(errorMessage(e, '삭제에 실패했어요. 다시 시도해 주세요.'));
      setDeleting(false);
    }
  };

  return (
    <div id="view-release" className="view">
      <div className="spaced-actions">
        <button type="button" className="link-btn aq-back-link" onClick={() => nav('/releases')}><Glyph name="arrow-left" size={15} className="aq-glyph-lead" />발매 목록</button>
      </div>

      <div className="aq-detail-hero" aria-label="발매 정보">
        <ReleaseCover id={rel.id} src={coverSrc} className="cover aq-detail-cover" />
        <div className="aq-detail-info">
          <div className="aq-detail-top">
            <span className="eyebrow">{kindLabel(d?.type) || '발매'}</span>
            <span className={`status-chip ${rel.status}`}>{STATUS_LABEL[rel.status] || rel.status}</span>
          </div>
          <h1>{rel.title || '제목 없는 발매'}</h1>
          <p className="aq-detail-artist">{rel.artist || '아티스트 미입력'}</p>
          <div className="aq-detail-facts">
            <span>{rel.tracks.length}곡</span>
            <span>{genre || '장르 미등록'}</span>
            <span>{rel.release_date || '발매일 미정'}</span>
          </div>
        </div>
      </div>

        <div className="row-actions aq-detail-actions">
        {(d?.application || !isDraft) && (
          <button type="button" className="button secondary" onClick={() => nav(`/releases/${encodeURIComponent(rel.id)}/application`)}>
            신청서 보기
          </button>
        )}
        {/* 보완하기는 아래 보완 카드 안에 — 위 버튼 줄은 모두 같은 옅은 버튼으로 */}
        {rel.status !== 'closed' && !rejected && !needsFix && (
          <button
            type="button" className="button secondary"
            onClick={() => nav(`/upload?edit=${encodeURIComponent(rel.id)}`)}
          >
            {editLabel}
          </button>
        )}
        {canWithdraw && (
          <button type="button" className="button secondary aq-withdraw-btn" onClick={handleWithdraw} disabled={withdrawing}>
            {withdrawing ? '취소하는 중' : '신청 취소'}
          </button>
        )}
        {isDraft && (
          <button type="button" className="button danger" onClick={handleDelete} disabled={deleting}>
            {deleting ? '삭제 중' : '삭제'}
          </button>
        )}
      </div>

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
          {STAGE_NOTE[rel.status] && <p className="aq-stage-note">{STAGE_NOTE[rel.status]}</p>}
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
        <section className="aq-fix-card is-rejected" aria-labelledby="aqRejectHead">
          <div className="aq-fix-card-head">
            <span className="aq-fix-icon" aria-hidden="true"><Glyph name="close" size={16} /></span>
            <div className="min-0">
              <h2 id="aqRejectHead">발매가 거절됐어요</h2>
              <p className="break" style={{ whiteSpace: 'pre-line' }}>{reviewNote
                || '담당자 검토 결과 이 발매는 배급할 수 없어요. 궁금한 점은 문의로 남겨 주세요.'}</p>
              <p className="small muted" style={{ marginTop: 8 }}>거절된 발매는 다시 접수할 수 없어요. 내용을 고쳐 새 발매로 신청해 주세요.</p>
            </div>
          </div>
        </section>
      )}

      {needsFix && (
        <section className="aq-fix-card" aria-labelledby="aqFixCardHead">
          <div className="aq-fix-card-head">
            <span className="aq-fix-icon" aria-hidden="true"><Glyph name="alert" size={18} /></span>
            <div className="min-0">
              <h2 id="aqFixCardHead">{fixes.length ? `보완 요청 ${fixes.length}건` : '보완이 필요해요'}</h2>
              <p>{fixes.length
                ? '항목을 누르면 고칠 칸으로 바로 이동해요. 다 고친 뒤 마지막 단계에서 다시 접수해 주세요.'
                : reviewNote
                  ? '담당자 의견을 확인하고 신청서를 고친 뒤 마지막 단계에서 다시 접수해 주세요.'
                  : '알림과 권리·보완 서류에서 요청 내용을 확인한 뒤 ‘보완하기’로 다시 접수해 주세요.'}</p>
            </div>
          </div>
          {fixes.length > 0 && (
            <CorrectionList
              releaseId={rel.id} corrections={fixes}
              trackIds={(d?.draftTracks ?? rel.tracks).map(t => t.id)}
              trackTitles={Object.fromEntries((d?.draftTracks ?? rel.tracks).map(t => [t.id, t.title]))}
            />
          )}
          {reviewNote && <ReviewNote text={reviewNote} />}
          {/* 무엇을 고칠지 먼저 읽고 나서 누르도록 목록 아래에 둔다 */}
          <button type="button" className="button aq-fix-go aq-fix-go-foot" onClick={() => nav(fixPath(rel.id, fixes[0]))}>보완하기</button>
        </section>
      )}

      <div className="tabs aq-tabs" role="tablist" aria-label="발매 상세 메뉴">
        {DETAIL_TABS.map(t => (
          <button
            key={t.value} type="button" role="tab" className="tab"
            aria-selected={tab === t.value}
            onClick={() => setTab(t.value)}
          >{t.label}</button>
        ))}
      </div>

      <div id="releaseDetailBody" className="aq-tab-panel" key={tab}>
        {tab === 'overview' && (
          <div className="split">
            <div>
              <h2 className="subhead">발매 정보</h2>
              <dl className="information">
                <div><dt>아티스트</dt><dd>{rel.artist || '미입력'}</dd></div>
                <div><dt>발매 유형</dt><dd>{kindLabel(d?.type) || '미입력'}</dd></div>
                <div><dt>주요 언어</dt><dd>{languageLabel(d?.language) || '미입력'}</dd></div>
                <div><dt>발매 예정일</dt><dd>{rel.release_date || '미정'}</dd></div>
                <div><dt>UPC / EAN</dt><dd>{d?.upc || '등록 전'}</dd></div>
                <div><dt>장르</dt><dd>{genre || '미입력'}</dd></div>
                <div><dt>레이블</dt><dd>{d?.label || '미입력'}</dd></div>
              </dl>
              <h2 className="subhead">앨범 소개</h2>
              <p className="muted small break">{d?.notes || '등록된 소개가 없어요.'}</p>
            </div>
            <div className="studio-album-aside aq-cover-card">
              <h2 className="subhead">커버아트</h2>
              <div className={`aq-cover-stage${coverSrc ? '' : ' is-empty'}`}>
                {coverSrc ? (
                  <>
                    {/* 커버 색이 카드 바탕으로 번지게 — 같은 이미지를 흐리게 깐다 */}
                    <img className="aq-cover-glow" src={coverSrc} alt="" aria-hidden="true" decoding="async" />
                    <img className="aq-detail-cover-large" src={coverSrc} alt={`${rel.title} 커버아트`} decoding="async" />
                  </>
                ) : (
                  <div className="aq-detail-cover-empty">
                    <Glyph name="music" size={22} />
                    <span>{d?.coverName ? '미리보기 없음' : '커버아트 없음'}</span>
                  </div>
                )}
              </div>
              <dl className="aq-cover-meta">
                {d?.coverName && <div><dt>파일</dt><dd className="break">{d.coverName}</dd></div>}
                <div><dt>최근 수정</dt><dd>{rel.updated_at ? localStamp(rel.updated_at) : localStamp(rel.created_at)}</dd></div>
              </dl>
            </div>
          </div>
        )}

        {tab === 'tracks' && (
          <>
            <h2 className="subhead">트랙 목록 · {rel.tracks.length}곡</h2>
            {rel.tracks.length ? (
              <div className="aq-catalog-cards aq-stagger">
                {rel.tracks.map((t, i) => (
                  <div key={t.id} className="aq-track-card">
                    <span className="aq-track-num" aria-hidden="true">{i + 1}</span>
                    <div className="min-0">
                      <span className="row-name">
                        {t.title || '곡명 없음'}{t.version ? ` (${t.version})` : ''}
                        {t.explicit && <em className="aq-explicit" title="Explicit">E</em>}
                      </span>
                      <span className="row-sub">
                        ISRC {t.isrc || '등록 전'} · 작곡 {t.composers || '미입력'} · 작사 {t.lyricists || '없음'}
                        {t.duration_ms ? ` · ${durationLabel(t.duration_ms)}` : ''}
                      </span>
                      <span className="row-sub">음원 파일: {t.audioName || '미등록'}</span>
                    </div>
                    <span className={`status-chip ${t.audioName ? 'ready' : 'draft'}`}>{t.audioName ? '파일 등록' : '파일 없음'}</span>
                  </div>
                ))}
              </div>
            ) : (
              <div className="empty-note">
                등록된 트랙이 없어요.<br />
                <button type="button" className="link-btn" onClick={() => nav(`/upload?edit=${encodeURIComponent(rel.id)}`)}>트랙 등록하기 <Glyph name="arrow-up-right" size={13} /></button>
              </div>
            )}
            <div className="notice" style={{ marginTop: 22 }}>
              음원 원본의 재첨부가 필요한 경우가 있어요. 제출 전 파일을 다시 확인해 주세요.
            </div>
          </>
        )}

        {tab === 'delivery' && (
          <div className="split">
            <div>
              <h2 className="subhead">배급 설정</h2>
              <dl className="information">
                <div><dt>희망 배급 지역</dt><dd>{d?.territories?.includes('WORLD') ? '전 세계' : '지정 안 함'}</dd></div>
                <div><dt>발매 예정일</dt><dd>{rel.release_date || '미정'}</dd></div>
                {d?.originalDate && <div><dt>최초 발매일</dt><dd>{d.originalDate}</dd></div>}
              </dl>
              <div className="tag-row">
                {d?.platforms?.length ? (
                  d.platforms.map(p => <span key={p} className="tag">{dspLabel(p)}</span>)
                ) : (
                  <span className="muted small">플랫폼 미선택</span>
                )}
              </div>
              {!!delivery.data?.length && (
                <>
                  <h2 className="subhead">플랫폼별 배급 진행</h2>
                  <ul className="aq-linked-docs">
                    {delivery.data.map(item => (
                      <li key={item.dsp}>
                        <div className="min-0">
                          <span>{dspLabel(item.slug)}</span>
                          {item.issues.filter(i => i.severity === 'BLOCKER').map(i => (
                            <p key={i.code} className="small muted">{DELIVERY_ISSUE[i.code] ?? '이 플랫폼 기준에 맞게 확인이 필요해요.'}</p>
                          ))}
                        </div>
                        <em>{DELIVERY_STAGE[item.stage] ?? item.stage}</em>
                      </li>
                    ))}
                  </ul>
                </>
              )}
              <h2 className="subhead">권리 확인</h2>
              <dl className="information">
                <div><dt>마스터 권리자</dt><dd>{d?.ownership || '미입력'}</dd></div>
                <div><dt>℗ 표기</dt><dd>{d?.phonogram || '미입력'}</dd></div>
                <div><dt>© 표기</dt><dd>{d?.copyright || '미입력'}</dd></div>
              </dl>
              <p className={`small ${rightsOk(d?.rightsChecks, d?.options) ? 'aq-ok-text' : 'muted'}`}>
                {rightsOk(d?.rightsChecks, d?.options) ? <><span className="aq-check-badge is-sm"><CheckIcon size={10} /></span> 신청서 권리 확인 항목 작성 완료</> : '권리 확인 항목을 보완해 주세요.'}
              </p>
            </div>
            <div className="surface">
              <h2 className="subhead">계약·증빙</h2>
              <ContractBundle
                compact releaseId={rel.id} releaseTitle={rel.title}
                agreement={docs.find(x => x.kind === 'agreements')}
                hasApplication={!!d?.application || !isDraft}
                onOpenAgreement={docId => nav(`/contracts?doc=${encodeURIComponent(docId)}`)}
              />
              {docs.some(x => x.kind === 'rights') && <>
                <h3 className="aq-bundle-sub">권리 서류</h3>
                <ul className="aq-linked-docs">
                  {docs.filter(x => x.kind === 'rights').map(doc => (
                    <li key={doc.id}>
                      <Link to="/rights">
                        <span className="min-0">{doc.title.replace(`${rel.title} · `, '')}</span>
                        <em>{docState(doc)}</em>
                      </Link>
                    </li>
                  ))}
                </ul>
              </>}
              <button className="button secondary aq-doc-manage" type="button" onClick={() => nav('/contracts')}>문서 관리 <Glyph name="arrow-up-right" size={14} /></button>
            </div>
          </div>
        )}

        {tab === 'history' && (
          <>
            <h2 className="subhead">변경 기록</h2>
            {d?.history?.length ? (
              <ol className="aq-timeline">
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
          </>
        )}
      </div>
    </div>
  );
}
