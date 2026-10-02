// 처리 이력 — 접수·검사·담당자 결정·처리 작업·플랫폼 전송/응답을 시간순으로. 종류별로 걸러 보고, 날짜별로 묶는다.
import { useState } from 'react';
import { staffApi, type ReleaseTimelineItem } from '../api/staff';
import { useAsync } from '../hooks/useAsync';
import { APPROVAL_STATUS, CHECK_STATUS, checkLabel, checkSummary, dspLabel, pick, shortId } from '../labels';
import { ErrorBox, Section, Skeleton, StatusChip } from '../ui';
import { Glyph } from './Glyph';

export const ACTION_KO: Record<string, string> = {
  'release.submitted': '발매 접수', 'stage1.decision': '1차 검사 결과', 'stage1.completed': '1차 검사 완료', 'stage2.decision': '2차 검사 결과',
  'stage2.pass': '2차 검사 통과', 'stage3.prepared': '배급 준비 완료', 'delivery.staged': '플랫폼별 전송 준비',
  'delivery.held': '배급 대기 (계약서 서명 전)', 'delivery.enqueued': '플랫폼 전송 예약', 'release.updated': '발매 정보 수정',
  'staff.approved': '담당자 승인', 'staff.correction_requested': '담당자 보완 요청', 'staff.rejected': '담당자 거절',
  'staff.approval_requested': '2차 승인 요청', 'staff.approval_granted': '2차 승인 완료', 'staff.approval_declined': '2차 승인 반려',
  'staff.approval_expired': '2차 승인 만료', 'staff.approval_cancelled': '2차 승인 요청 종료',
  'staff.proof_requested': '권리 증빙 요청', 'release.withdrawn': '신청 취소', 'staff.identifiers_reissue': '식별자 재발급 요청', 'staff.document_reviewed': '신청서 처리',
  'staff.document_viewed': '서류 원본 열람', 'staff.delivery_decided': '플랫폼 배급 결정', 'staff.delivery_restaged': '플랫폼 배급 다시 준비',
  'staff.delivery_live_recorded': '서비스 시작 기록', 'staff.dsp_contract_route': '계약 경로 지정',
  'stage1.gave_up': '1차 검사 중단', 'stage2.gave_up': '2차 검사 중단', 'stage3.gave_up': '배급 준비 중단',
  'stage2.checks_recorded': '2차 검사 결과 저장', 'stage2.technical_retry': '2차 검사 다시 시도', 'stage3.return_to_s2': '2차 검사로 되돌림',
'release.title': '발매 제목 수정', 'release.artist': '아티스트 수정',
  'release.track_added': '트랙 추가', 'release.track_updated': '트랙 수정', 'release.track_archived': '트랙 삭제', 'release.credits_replaced': '크레딧 수정',
  'asset.registered': '파일 등록', 'asset.upload_cancelled': '업로드 취소', 'consent.created': '배급 동의',
  'job.succeeded': '처리 작업 완료', 'job.retry': '처리 작업 다시 시도', 'job.park': '처리 작업 대기', 'job.dead_letter': '처리 작업 실패',
  'delivery.auto_approved': '플랫폼 자동 승인', 'delivery.live_recorded': '서비스 시작', 'delivery.takedown': '배급 중단',
  'portal.application.signed': '신청서 서명', 'portal.document.created': '서류 생성', 'portal.document.signed': '서류 서명',
  'portal.document.proof_submitted': '증빙 제출', 'portal.rights_document.signed': '권리 서류 서명',
};
const JOB_LABEL: Record<string, string> = {
  'asset.analyze': '음원 분석', stage1: '1차 검사', stage2: '2차 검사',
  prepare_release: '배급 준비', 'delivery.stage': '플랫폼별 배급 준비', 'delivery.send': '플랫폼 전송',
  'delivery.enqueue': '플랫폼 전송 예약', 'delivery.poll': '배급 상태 확인', 'delivery.ack': '플랫폼 응답 처리',
  'delivery.reconcile': '배급 결과 확인', 'delivery.takedown': '배급 중단',
};
const JOB_STATUS: Record<string, string> = {
  QUEUED: '대기', RUNNING: '진행 중', SUCCEEDED: '완료', FAILED: '실패', DEAD: '처리 실패', DEAD_LETTER: '처리 실패', RETRY: '재시도 대기',
};

const OUTCOME: Record<string, string> = {
  SENT: '전송함', DELIVERED: '전달 완료', ACCEPTED: '플랫폼 접수', PROCESSING: '플랫폼 처리 중', LIVE: '서비스 중 (LIVE)',
  REJECTED: '플랫폼 반려', ERROR: '전송 오류', FAILED: '전송 실패', TAKEN_DOWN: '서비스 중단',
};

type Kind = 'staff' | 'check' | 'job' | 'dsp' | 'release';
const KIND_LABEL: Record<Kind, string> = { staff: '담당자', check: '검사', job: '처리 작업', dsp: '플랫폼', release: '발매' };
const KINDS: Kind[] = ['staff', 'check', 'job', 'dsp', 'release'];

export function timelineKind(t: ReleaseTimelineItem): Kind {
  if (t.source === 'staff_decision' || t.kind.startsWith('staff.')) return 'staff';
  if (t.source === 'check' || (t.source === 'audit' && t.kind.startsWith('stage'))) return 'check';
  if (t.source === 'job') return 'job';
  if (t.source === 'dsp_request' || t.source === 'dsp_ack' || t.kind.startsWith('delivery.')) return 'dsp';
  return 'release';
}

/** 점 색 — 실패·차단은 빨강, 확인 필요·재시도는 주황 */
function tone(t: ReleaseTimelineItem): 'bad' | 'warn' | '' {
  const s = String(t.detail.status ?? t.detail.outcome ?? '');
  if (/FAIL|DEAD|BLOCK|REJECT|ERROR|CORRECTION/.test(s)) return 'bad';
  if (/REVIEW|RETRY|HOLD/.test(s)) return 'warn';
  return '';
}

const timeFmt = new Intl.DateTimeFormat('ko-KR', { hour: '2-digit', minute: '2-digit', hour12: false, timeZone: 'Asia/Seoul' });
const dayFmt = new Intl.DateTimeFormat('ko-KR', { month: 'long', day: 'numeric', weekday: 'short', timeZone: 'Asia/Seoul' });
const fmt = (f: Intl.DateTimeFormat, iso: string) => { const d = new Date(iso); return Number.isNaN(d.getTime()) ? iso : f.format(d); };

/** 감사 기록의 사유 코드 → 한 줄 설명 (‘PASS:2’, ‘REVIEW:S2_META_CREDITS,…’, 보완 항목 코드 목록) */
export function reasonSummary(action: string, reason: unknown): string {
  if (typeof reason !== 'string' || !reason) return '';
  const labels = (codes: string) => [...new Set(codes.split(',').map(c => c.trim()).filter(Boolean).map(c => checkLabel(c.split('@')[0])))].join(', ');
  const [head, rest] = reason.includes(':') ? [reason.slice(0, reason.indexOf(':')), reason.slice(reason.indexOf(':') + 1)] : [reason, ''];
  if (action === 'staff.correction_requested') return `보완 항목: ${labels(reason)}`;
  if (head === 'PASS' && /^\d+$/.test(rest)) return `확인 필요 ${rest}건 통과 처리`;
  if (head === 'REVIEW' && rest) return `확인 필요: ${labels(rest)}`;
  if ((head === 'CORRECTION' || head === 'BLOCKED' || head === 'FAIL') && rest) return `문제: ${labels(rest)}`;
  return '';
}

/** 합쳐도 되는 반복: 상태·결과가 같은 기록 (검사 결과가 바뀌었으면 따로 보여 준다) */
function sameDetail(a: ReleaseTimelineItem, b: ReleaseTimelineItem): boolean {
  const k = (t: ReleaseTimelineItem) => [t.detail.status, t.detail.outcome, t.detail.approval, t.detail.reason].map(v => String(v ?? '')).join('|');
  return k(a) === k(b);
}

export function TimelineRow({ item: t, repeat = 1 }: { item: ReleaseTimelineItem; repeat?: number }) {
  const kind = timelineKind(t);
  let title: string;
  let summary = '';
  if (t.source === 'audit') {
    title = ACTION_KO[t.kind] ?? '처리 기록';
    summary = reasonSummary(t.kind, t.detail.reason);
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
    summary = typeof t.detail.outcome === 'string' ? OUTCOME[t.detail.outcome] ?? t.detail.outcome : '';
  }
  const actor = t.source === 'audit'
    ? (typeof t.detail.actor_service === 'string' ? '시스템' : typeof t.detail.actor_user_id === 'string' ? (kind === 'staff' ? `담당자 ${shortId(t.detail.actor_user_id)}` : '아티스트') : '')
    : '';
  return <li className={['is-' + kind, tone(t) && `is-${tone(t)}`].filter(Boolean).join(' ')}>
    <i aria-hidden="true" />
    <div className="adm-min">
      <div className="adm-tl-top">
        <b>{title}</b>
        {repeat > 1 && <span className="adm-tl-repeat">×{repeat}</span>}
        {t.source === 'check' && <StatusChip value={pick(CHECK_STATUS, String(t.detail.status))} />}
        {t.source === 'staff_decision' && <StatusChip value={pick(APPROVAL_STATUS, String(t.detail.approval))} />}
      </div>
      <small>{fmt(timeFmt, t.at)} · {KIND_LABEL[kind]}{actor && ` · ${actor}`}</small>
      {summary && <p className="adm-tl-summary">{summary}</p>}
      <details className="adm-more"><summary>기록 원문</summary><span className="adm-code">{t.kind}</span><pre className="adm-raw">{JSON.stringify(t.detail, null, 2)}</pre></details>
    </div>
  </li>;
}

export function ReviewTimeline({ releaseId, revisionId, refreshTick }: { releaseId: string; revisionId: string | null; refreshTick: number }) {
  const [limit, setLimit] = useState(100);
  const [filter, setFilter] = useState<Kind | 'all'>('all');
  const [expanded, setExpanded] = useState(false);
  const { data, error, loading, reload } = useAsync(() => staffApi.timeline(releaseId, limit), [releaseId, revisionId, refreshTick, limit]);
  const items = data?.items.slice().reverse() ?? [];
  const counts = items.reduce<Partial<Record<Kind, number>>>((m, t) => { const k = timelineKind(t); m[k] = (m[k] ?? 0) + 1; return m; }, {});
  const filtered = filter === 'all' ? items : items.filter(t => timelineKind(t) === filter);
  // 최근 것부터 몇 건만 — 나머지는 ‘더 보기’로 (긴 이력이 시트를 밀어내지 않게)
  const PREVIEW = 6;
  const shown = expanded || filtered.length <= PREVIEW + 2 ? filtered : filtered.slice(0, PREVIEW);
  // 날짜별 묶음 (최신 날짜부터). 바로 이어지는 같은 기록(같은 작업 반복 등)은 한 줄로 합쳐 개수만 보인다
  const groups: { day: string; items: { item: ReleaseTimelineItem; repeat: number }[] }[] = [];
  for (const t of shown) {
    const d = fmt(dayFmt, t.at);
    let last = groups[groups.length - 1];
    if (last?.day !== d) { last = { day: d, items: [] }; groups.push(last); }
    const prev = last.items[last.items.length - 1];
    if (prev && prev.item.source === t.source && prev.item.kind === t.kind && sameDetail(prev.item, t)) prev.repeat += 1;
    else last.items.push({ item: t, repeat: 1 });
  }
  return <Section title="처리 이력" meta={data ? `${items.length}건${data.truncated ? ` · 최근 ${limit}건` : ''}` : '검사·담당자 결정·배급 작업·플랫폼 응답'}>
    {error && <ErrorBox message={error} onRetry={reload} />}
    {loading && !data ? <Skeleton rows={3} /> : <>
      {items.length > 0 && (
        <div className="adm-tl-filter" role="group" aria-label="이력 종류">
          <button type="button" className={filter === 'all' ? 'is-on' : ''} aria-pressed={filter === 'all'} onClick={() => { setFilter('all'); setExpanded(false); }}>전체 <span>{items.length}</span></button>
          {KINDS.filter(k => counts[k]).map(k => (
            <button key={k} type="button" className={filter === k ? 'is-on' : ''} aria-pressed={filter === k} onClick={() => { setFilter(k); setExpanded(false); }}>{KIND_LABEL[k]} <span>{counts[k]}</span></button>
          ))}
        </div>
      )}
      {!items.length && <p className="small muted">기록된 처리 이력이 없어요.</p>}
      {groups.map(g => (
        <div key={g.day} className="adm-tl-day">
          <h3>{g.day}</h3>
          <ol className="adm-timeline">{g.items.map(({ item: t, repeat }, i) => <TimelineRow key={`${t.at}-${t.source}-${t.kind}-${i}`} item={t} repeat={repeat} />)}</ol>
        </div>
      ))}
      {shown.length < filtered.length && (
        <button type="button" className="adm-tl-more" onClick={() => setExpanded(true)}>이전 기록 {filtered.length - shown.length}건 더 보기<Glyph name="chevron-right" size={12} /></button>
      )}
      {(expanded || filtered.length <= PREVIEW + 2) && data?.truncated && limit < 2000 && <button type="button" className="adm-btn soft small" disabled={loading} onClick={() => setLimit(n => Math.min(n * 5, 2000))}>이전 기록 더 보기</button>}
    </>}
  </Section>;
}
