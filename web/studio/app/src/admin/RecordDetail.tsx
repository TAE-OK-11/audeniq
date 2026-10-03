// ‘자세히’ — 시스템 기록(검사 원문·처리 이력)을 영어 키·JSON 그대로 보여 주지 않고 한국어 항목으로 풀어 보여 준다.
// 알아볼 수 있는 항목만 고르고, 의미를 알 수 없는 영어 원문은 숨긴다 (원문 전체는 서버 감사 기록에 남아 있다).
import type { ReactNode } from 'react';
import { APPROVAL_STATUS, CHECK_STATUS, READINESS, pick, shortId, when } from './labels';

const HANGUL = /[가-힣]/;

const JOB_STATUS: Record<string, string> = {
  QUEUED: '대기', RUNNING: '진행 중', SUCCEEDED: '완료', FAILED: '실패', DEAD: '처리 실패', DEAD_LETTER: '처리 실패', RETRY: '다시 시도 대기', PARKED: '보류',
};
const OUTCOME: Record<string, string> = {
  SENT: '전송함', DELIVERED: '전달 완료', ACCEPTED: '플랫폼 접수', PROCESSING: '플랫폼 처리 중', LIVE: '서비스 중',
  REJECTED: '플랫폼 반려', ERROR: '전송 오류', FAILED: '전송 실패', TAKEN_DOWN: '서비스 중단',
};

/** 상태 코드 → 한국어 (검사·작업·결정·전송 결과 어느 쪽이든) */
function statusKo(v: string): string {
  return CHECK_STATUS[v]?.[0] ?? JOB_STATUS[v] ?? OUTCOME[v] ?? APPROVAL_STATUS[v]?.[0] ?? READINESS[v]?.[0] ?? '';
}

type Fmt = (v: unknown) => ReactNode;
const id: Fmt = v => (typeof v === 'string' && v ? <span className="adm-id">{shortId(v)}</span> : null);
const text: Fmt = v => (typeof v === 'string' && HANGUL.test(v) ? v : null);
const status: Fmt = v => (typeof v === 'string' ? statusKo(v) || null : null);
const count = (unit: string): Fmt => v => (typeof v === 'number' || (typeof v === 'string' && /^\d+$/.test(v)) ? `${v}${unit}` : null);
const time: Fmt = v => (typeof v === 'string' && v ? when(v) : null);
const yes: Fmt = v => (typeof v === 'boolean' ? (v ? '예' : '아니요') : null);

/** 처리 이력 한 줄의 detail 키 → [한국어 이름, 표시 방법] — 목록 순서대로 보여 준다 */
const FIELDS: [string, string, Fmt][] = [
  ['status', '상태', status],
  ['outcome', '결과', status],
  ['approval', '결정', status],
  ['readiness', '준비 상태', status],
  ['attempts', '실행 횟수', count('회')],
  ['attempt', '전송 차수', count('번째')],
  ['applied', '반영', yes],
  ['run_at', '다음 실행', time],
  ['note', '메모', text],
  ['last_error', '오류', v => (v ? '처리 중 오류가 있었어요. 자동으로 다시 시도해요.' : null)],
  ['actor_user_id', '처리한 사람', id],
  ['resource_id', '대상', id],
  ['revision_id', '수정본', id],
  ['package_id', '배급 묶음', id],
  ['job_id', '작업 번호', id],
  ['delivery_job_id', '전송 작업', id],
  ['partner_message_id', '플랫폼 메시지', id],
  ['event_id', '응답 번호', id],
  ['request_id', '요청 번호', id],
];

/** 검사 원문(영어 문장)에서 알아볼 수 있는 측정값만 */
const MEASURES: [RegExp, string, (m: RegExpMatchArray) => string][] = [
  [/integrated_lufs=(-?[\d.]+)/, '통합 음량', m => `${m[1]} LUFS`],
  [/true_peak_dbtp=(-?[\d.]+)/, '최대 피크', m => `${m[1]} dBTP`],
  [/peak_dbfs=(-?[\d.]+)/, '최대 레벨', m => `${m[1]} dBFS`],
  [/sample_rate=(\d+)/, '샘플레이트', m => `${(Number(m[1]) / 1000).toLocaleString('ko-KR')} kHz`],
  [/(?:bits_per_sample|bits)=(\d+)/, '비트 깊이', m => `${m[1]}bit`],
  [/channels=(\d+)/, '채널', m => (m[1] === '1' ? '모노' : m[1] === '2' ? '스테레오' : `${m[1]}채널`)],
  [/(?:duration_secs|duration)=([\d.]+)/, '길이', m => { const s = Math.round(Number(m[1])); return `${Math.floor(s / 60)}분 ${s % 60}초`; }],
  [/codec=([\w.-]+)/, '코덱', m => m[1].toUpperCase()],
  [/similar to (\d+) asset/, '비슷한 음원', m => `${m[1]}개`],
  [/shift=(-?[\d.]+)/, '시간 차', m => `${m[1]}초`],
  [/track=(\d+)/, '트랙', m => `${m[1]}번`],
];

export function measuresOf(detail: string | null | undefined): [string, string][] {
  if (!detail) return [];
  const out: [string, string][] = [];
  for (const [re, label, fmt] of MEASURES) {
    const m = detail.match(re);
    if (m) out.push([label, fmt(m)]);
  }
  return out;
}

function List({ rows }: { rows: [string, ReactNode][] }) {
  if (!rows.length) return <p className="adm-detail-empty">더 볼 내용이 없어요.</p>;
  return (
    <dl className="adm-detail-list">
      {rows.map(([k, v]) => <div key={k}><dt>{k}</dt><dd>{v}</dd></div>)}
    </dl>
  );
}

/** 처리 이력 한 줄에서 보여 줄 항목 — skip: 이미 줄에 보이는 키(상태 칩·요약과 겹치지 않게) */
export function recordRows(detail: Record<string, unknown>, skip: string[] = []): [string, ReactNode][] {
  const rows: [string, ReactNode][] = [];
  for (const [key, label, fmt] of FIELDS) {
    if (skip.includes(key) || !(key in detail)) continue;
    const v = fmt(detail[key]);
    if (v != null && v !== '') rows.push([label, v]);
  }
  if (typeof detail.detail === 'string') rows.push(...measuresOf(detail.detail));
  return rows;
}

export function RecordDetail({ rows }: { rows: [string, ReactNode][] }) {
  return <List rows={rows} />;
}

/** 검사 카드 */
export function CheckDetail({ c }: { c: { check_code: string; stage?: number | null; status: string; original_status?: string | null; detail: string | null } }) {
  const rows: [string, ReactNode][] = [];
  if (c.stage) rows.push(['검사 단계', `${c.stage}차 검사`]);
  rows.push(['결과', pick(CHECK_STATUS, c.status)[0]]);
  if (c.original_status && c.original_status !== c.status) rows.push(['시스템 판정', `${pick(CHECK_STATUS, c.original_status)[0]} → 담당자 결정 반영`]);
  rows.push(...measuresOf(c.detail));
  if (c.detail && HANGUL.test(c.detail)) rows.push(['메모', c.detail]);
  return <List rows={rows} />;
}
