// 발매 한 건의 계약 서류 묶음 — 1. 배급 신청서(접수 때 서명) → 2. 배급 계약서(심사 후 서명)
// 계약서 화면과 발매 상세 ‘배급·권리’ 탭에서 같은 묶음으로 보여 준다.
import { useNavigate } from '../lib/router';
import { docState, type DocRecord } from '../store/docs';
import { niceDate, stripSampleSuffix } from '../lib/format';
import { CheckIcon } from './Check';
import { Glyph } from './Glyph';

export type BundleStage = 'to-sign' | 'review' | 'needs' | 'signed' | 'waiting';

/** 묶음 전체 상태 — 정렬과 머리 표시에 쓴다 */
export function bundleStage(agreement: DocRecord | null | undefined): BundleStage {
  if (!agreement) return 'waiting';
  if (agreement.localSignatureAt) return 'signed';
  if (agreement.reviewStatus === 'needs') return 'needs';
  if (agreement.reviewStatus === 'approved') return 'to-sign';
  return 'review';
}

const STAGE_LABEL: Record<BundleStage, string> = {
  'to-sign': '계약서 서명 필요',
  review: '담당자 검토 중',
  needs: '보완 필요',
  signed: '계약 체결 완료',
  waiting: '계약서 준비 전',
};

export function ContractBundle({ releaseId, releaseTitle, agreement, hasApplication = true, onOpenAgreement, compact = false }: {
  releaseId: string;
  releaseTitle: string;
  agreement: DocRecord | null | undefined;
  /** 접수한 신청서가 있는지 (작성 중인 발매는 없음) */
  hasApplication?: boolean;
  onOpenAgreement: (id: string) => void;
  /** 발매 상세 안: 발매명 머리 없이 */
  compact?: boolean;
}) {
  const nav = useNavigate();
  const stage = bundleStage(agreement);
  const canOpen = !!agreement && (stage === 'to-sign' || stage === 'signed');
  return (
    <article className={`aq-bundle is-${stage}${compact ? ' is-compact' : ''}`} aria-label={`${releaseTitle} 계약 서류`}>
      {!compact && (
        <header className="aq-bundle-head">
          <div className="min-0">
            <strong>{stripSampleSuffix(releaseTitle) || '제목 없는 발매'}</strong>
            {agreement?.created && <span>{niceDate(agreement.created)} 접수</span>}
          </div>
          <em>{STAGE_LABEL[stage]}</em>
        </header>
      )}
      <ol className="aq-bundle-docs">
        <li className={hasApplication ? 'is-done' : ''}>
          <span className="aq-bundle-no" aria-hidden="true">{hasApplication ? <CheckIcon size={12} /> : 1}</span>
          <div className="min-0">
            <strong>배급 신청서</strong>
            <span>{hasApplication ? '접수할 때 서명 완료' : '발매를 접수할 때 서명해요'}</span>
          </div>
          {hasApplication && (
            <button type="button" className="link-btn" onClick={() => nav(`/releases/${encodeURIComponent(releaseId)}/application`)}>
              신청서 보기
            </button>
          )}
        </li>
        <li className={stage === 'signed' ? 'is-done' : stage === 'to-sign' ? 'is-current' : ''}>
          <span className="aq-bundle-no" aria-hidden="true">{stage === 'signed' ? <CheckIcon size={12} /> : stage === 'to-sign' ? <Glyph name="pencil" size={12} /> : 2}</span>
          <div className="min-0">
            <strong>배급 계약서</strong>
            <span>{agreement
              ? stage === 'review' ? '담당자 검토가 끝나면 서명할 수 있어요'
                : stage === 'needs' ? (agreement.reviewNote || '신청 내용을 보완해 다시 접수해 주세요')
                  : docState(agreement)
              : '신청서를 접수하면 준비돼요'}</span>
          </div>
          {canOpen && (
            <button type="button" className={stage === 'to-sign' ? 'button aq-bundle-sign' : 'link-btn'} onClick={() => onOpenAgreement(agreement!.id)}>
              {stage === 'to-sign' ? '확인하고 서명' : '계약서 보기'}
            </button>
          )}
        </li>
      </ol>
    </article>
  );
}
