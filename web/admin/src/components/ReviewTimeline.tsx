import { useState } from 'react';
import { staffApi, type ReleaseTimelineItem } from '../api/staff';
import { useAsync } from '../hooks/useAsync';
import { APPROVAL_STATUS, CHECK_STATUS, checkLabel, checkSummary, dspLabel, pick, when } from '../labels';
import { ErrorBox, Section, Skeleton, StatusChip } from '../ui';

export const ACTION_KO: Record<string, string> = {
  'release.submitted': '발매 접수', 'stage1.decision': '1차 검사 결과', 'stage1.completed': '1차 검사 완료', 'stage2.decision': '2차 검사 결과',
  'stage2.pass': '2차 검사 통과', 'stage3.prepared': '배급 준비 완료', 'delivery.staged': '플랫폼별 전송 준비',
  'delivery.held': '배급 대기 (계약서 서명 전)', 'delivery.enqueued': '플랫폼 전송 예약', 'release.updated': '발매 정보 수정',
  'staff.approved': '담당자 승인', 'staff.correction_requested': '담당자 보완 요청', 'staff.rejected': '담당자 거절',
  'staff.approval_requested': '2차 승인 요청', 'staff.approval_granted': '2차 승인 완료', 'staff.approval_declined': '2차 승인 반려',
  'staff.approval_expired': '2차 승인 만료', 'staff.approval_cancelled': '2차 승인 요청 종료',
  'staff.proof_requested': '권리 증빙 요청', 'release.withdrawn': '신청 취소', 'staff.identifiers_reissue': '식별자 재발급 요청', 'staff.document_reviewed': '신청서 처리',
};
const JOB_LABEL: Record<string, string> = {
  'asset.analyze': '음원 분석', stage1: '1차 검사', stage2: '2차 검사',
  prepare_release: '배급 준비', 'delivery.stage': '플랫폼별 배급 준비', 'delivery.send': '플랫폼 전송',
};
const JOB_STATUS: Record<string, string> = {
  QUEUED: '대기', RUNNING: '진행 중', SUCCEEDED: '완료', FAILED: '실패', DEAD: '처리 실패', RETRY: '재시도 대기',
};

export function TimelineRow({ item: t }: { item: ReleaseTimelineItem }) {
  let title: string;
  let summary = '';
  if (t.source === 'audit') {
    title = ACTION_KO[t.kind] ?? '처리 기록';
  } else if (t.source === 'job') {
    title = `${JOB_LABEL[t.kind] ?? '처리 작업'} 등록`;
    summary = `현재 상태: ${JOB_STATUS[String(t.detail.status)] ?? '확인 중'} · 실행 ${t.detail.attempts ?? 0}회`;
    if (t.detail.last_error) summary += ' · 처리 오류 기록 있음';
  } else if (t.source === 'check') {
    title = checkLabel(t.kind);
    summary = checkSummary({ check_code: t.kind, detail: typeof t.detail.detail === 'string' ? t.detail.detail : null });
  } else if (t.source === 'staff_decision') {
    title = `${dspLabel(t.kind)} 배급 결정`;
    summary = typeof t.detail.note === 'string' ? t.detail.note : '';
  } else {
    title = `${dspLabel(t.kind)} ${t.source === 'dsp_request' ? '전송 기록' : '응답 수신'}`;
    summary = typeof t.detail.outcome === 'string' ? t.detail.outcome : '';
  }
  return <li className={t.source === 'staff_decision' || t.kind.startsWith('staff.') ? 'is-staff' : ''}>
    <i aria-hidden="true" />
    <div>
      <b>{title}</b>
      {t.source === 'check' && <StatusChip value={pick(CHECK_STATUS, String(t.detail.status))} />}
      {t.source === 'staff_decision' && <StatusChip value={pick(APPROVAL_STATUS, String(t.detail.approval))} />}
      <small>{when(t.at)}</small>
      {summary && <p className="small">{summary}</p>}
      <details className="adm-more"><summary>상세 보기</summary><span className="adm-code">{t.kind}</span><pre className="adm-raw">{JSON.stringify(t.detail, null, 2)}</pre></details>
    </div>
  </li>;
}

export function ReviewTimeline({ releaseId, revisionId, refreshTick }: { releaseId: string; revisionId: string | null; refreshTick: number }) {
  const [limit, setLimit] = useState(100);
  const { data, error, loading, reload } = useAsync(() => staffApi.timeline(releaseId, limit), [releaseId, revisionId, refreshTick, limit]);
  return <Section title="처리 이력" meta="검사·담당자 결정·배급 작업·플랫폼 응답">
    {error && <ErrorBox message={error} onRetry={reload} />}
    {loading && !data ? <Skeleton rows={3} /> : <>
      {!data?.items.length && <p className="small muted">기록된 처리 이력이 없어요.</p>}
      <ol className="adm-timeline">{data?.items.slice().reverse().map((t, i) => <TimelineRow key={`${t.at}-${i}`} item={t} />)}</ol>
      {data?.truncated && <p className="small muted">최근 {limit}건을 표시하고 있어요.</p>}
      {data?.truncated && limit < 2000 && <button type="button" className="adm-btn soft small" disabled={loading} onClick={() => setLimit(n => Math.min(n * 5, 2000))}>이전 기록 더 보기</button>}
    </>}
  </Section>;
}
