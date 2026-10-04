// 체험(목) 빌드용 업무 API — 서버 없이 업무 화면을 확인할 수 있게 메모리 예시 데이터로 동작한다.
// 규칙(SLA 기한·escalation·반복 업무·업무함 모으기)은 crates/core/src/work.rs와 같게 흉내 낸다.
import { ApiError, messageForCode } from '../../api/errors';
import { DSP_NAME } from '../dspReqs';
import { DEFAULT_SLA, escalate, slaDueAt, slaState } from './sla';
import type {
  Analytics, Contact, ContactActivity, ContactDetail, ContactInput, DspOps, EntityRef, ExceptionAction, InboxItem,
  NoteInput, Priority, SlaPolicy, StaffPerson, TaskEvent, TaskInput, TaskPatch, TaskType, WorkException, WorkNote,
  WorkSummary, WorkTask, ContactKind, ExceptionKind, ExceptionStatus, InboxKind, EntityKind, ActivityKind, DspOpsPatch,
} from './types';
import type { TaskQuery, WorkApi } from './api';

const ME = 'staff-me-0001';
const KIM = 'staff-kim-0002';
const TAE = 'staff-tae-0003';
const PARK = 'staff-park-0004';
const people: StaffPerson[] = [
  { user_id: ME, email: 'staff@audeniq.com', name: '나 (staff)', role: 'ADMIN' },
  { user_id: KIM, email: 'kim@audeniq.com', name: '김민준', role: 'REVIEWER' },
  { user_id: TAE, email: 'taeyeong@audeniq.com', name: 'Taeyeong', role: 'OPERATOR' },
  { user_id: PARK, email: 'park@audeniq.com', name: '박서연', role: 'SUPPORT' },
];

const now = Date.now();
const H = 3_600_000;
const iso = (hoursAgo: number) => new Date(now - hoursAgo * H).toISOString();
const wait = <T>(v: T) => new Promise<T>(r => setTimeout(() => r(structuredClone(v)), 160));
const fail = (code: string, status = 422): never => { throw new ApiError(messageForCode(code, status), status, code); };
const uid = () => Math.random().toString(16).slice(2, 10);

let policies: SlaPolicy[] = structuredClone(DEFAULT_SLA);
const policyOf = (t: TaskType) => policies.find(p => p.type === t);

// ---------- 업무 ----------
let seq = 1040;
const tasks: WorkTask[] = [];
const events: Record<string, TaskEvent[]> = {};

function refresh(t: WorkTask) {
  const p = policyOf(t.type);
  t.sla_state = slaState(t, p?.warn_ratio ?? 0.25);
  const e = escalate(t.sla_state, t.priority_raised_from ?? t.priority, p);
  if (t.status !== 'DONE' && t.status !== 'CANCELLED') {
    if (e.priority !== t.priority && e.priority !== (t.priority_raised_from ?? t.priority)) {
      t.priority_raised_from ??= t.priority;
      t.priority = e.priority;
    }
    t.escalation_level = e.level;
  }
  return t;
}

function mk(hoursAgo: number, v: Partial<WorkTask> & Pick<WorkTask, 'title' | 'type'>): WorkTask {
  const created = iso(hoursAgo);
  const t: WorkTask = {
    id: `t-${uid()}`, key: `WRK-${seq++}`, description: '', status: 'TODO', priority: 'NORMAL',
    assignee_id: ME, reviewer_id: null, due_at: null, sla_due_at: slaDueAt(created, policyOf(v.type)), sla_state: 'NONE',
    escalation_level: 0, priority_raised_from: null, recurrence: null, related: [], tags: [],
    created_by: KIM, created_at: created, updated_at: created, assigned_at: created, started_at: null, completed_at: null,
    note_count: 0, row_version: 1,
    ...v,
  };
  events[t.id] = [{ at: created, actor_id: t.created_by, action: 'CREATED', detail: null }];
  if (t.status !== 'TODO') events[t.id].push({ at: iso(Math.max(0, hoursAgo - 1)), actor_id: t.assignee_id, action: 'STATUS', detail: t.status });
  tasks.push(refresh(t));
  return t;
}

const R = (id: string, label: string): EntityRef => ({ kind: 'release', id, label });
mk(1.6, { title: 'Paper Moon 긴급 송출 — 멜론 재전송', type: 'URGENT_DELIVERY', priority: 'HIGH', status: 'IN_PROGRESS', started_at: iso(1.2), related: [R('r4d5e6f7', 'Paper Moon'), { kind: 'dsp', id: 'D-1', label: '멜론' }], description: '아티스트 공연 일정 때문에 오늘 18시 전 멜론 노출이 필요해요. 전송 실패 원인(ERN 버전) 고친 뒤 재전송.', tags: ['긴급'] });
mk(30, { title: 'Blue Hour 샘플 사용 허락서 권리 검토', type: 'RIGHTS', assignee_id: ME, reviewer_id: KIM, related: [R('r2b3c4d5', 'Blue Hour'), { kind: 'document', id: 'doc-2', label: '샘플 사용 허락서' }], description: '허락서 원권리자 서명·사용 범위(전 세계, 스트리밍) 확인.' });
mk(20, { title: 'Lumi Records 배급 계약 갱신 검토', type: 'CONTRACT', status: 'IN_REVIEW', reviewer_id: ME, assignee_id: KIM, started_at: iso(18), related: [{ kind: 'contact', id: 'c-lumi', label: 'Lumi Records' }], description: '수수료 8% → 7% 조정 요청. 최소 지급액 조건 변경 여부 검토.' });
mk(52, { title: 'Spotify 메타데이터 반려 문의 회신', type: 'DSP_INQUIRY', priority: 'HIGH', assignee_id: TAE, related: [{ kind: 'dsp', id: 'D-5', label: 'Spotify' }, { kind: 'exception', id: 'ex-2', label: 'Afterglow 반려' }] });
mk(6, { title: '9월 정산 리포트 대사 — Apple Music 차액 확인', type: 'SETTLEMENT', assignee_id: ME, related: [{ kind: 'dsp', id: 'D-6', label: 'Apple Music' }, { kind: 'exception', id: 'ex-5', label: '정산 불일치' }], tags: ['월말'] });
mk(70, { title: '커버 무단 사용 신고 법무 대응', type: 'LEGAL', priority: 'HIGH', status: 'BLOCKED', assignee_id: ME, reviewer_id: KIM, started_at: iso(60), related: [R('r5e6f7a8', '고요한 밤의 노래')], description: '신고자 측 원본 증빙 요청함. 답변 오기 전까지 막힘.' });
mk(3, { title: '주간 DSP 장애 리포트 작성', type: 'GENERAL', priority: 'LOW', assignee_id: TAE, recurrence: { freq: 'WEEKLY', interval: 1, next_at: new Date(now + 4 * 24 * H).toISOString() }, tags: ['반복'] });
mk(26, { title: '새벽의 온도 발매 일정 문의 답변', type: 'SUPPORT', assignee_id: PARK, related: [{ kind: 'inquiry', id: 'iq-1', label: '심사가 얼마나 걸리나요?' }, R('r1a2b3c4', '새벽의 온도')] });
mk(96, { title: '신규 레이블 온보딩 서류 확인', type: 'CONTRACT', status: 'DONE', assignee_id: ME, started_at: iso(90), completed_at: iso(40), related: [{ kind: 'contact', id: 'c-nova', label: 'Nova Sound' }] });
mk(2, { title: '매일 아침 예외 큐 비우기', type: 'GENERAL', priority: 'NORMAL', assignee_id: ME, recurrence: { freq: 'DAILY', interval: 1, next_at: new Date(now + 20 * H).toISOString() }, tags: ['반복'] });

// ---------- 메모 ----------
const notes: WorkNote[] = [
  { id: 'n-1', target_kind: 'task', target_id: tasks[2].id, author_id: KIM, body: '@나 (staff) 수수료 조정 폭은 지난 분기 다른 레이블 조건이랑 맞춰 주세요. 비교표 첨부해요.', mentions: [ME], attachments: [{ id: 'att-1', name: '레이블_수수료_비교.xlsx', size: 48_211, content_type: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet' }], resolved: false, resolved_by: null, resolved_at: null, created_at: iso(5) },
  { id: 'n-2', target_kind: 'release', target_id: 'r2b3c4d5', author_id: TAE, body: '@나 (staff) 이 발매 Spotify에서 피처링 표기 때문에 한 번 반려됐던 이력 있어요. 심사 때 같이 봐 주세요.', mentions: [ME], attachments: [], resolved: false, resolved_by: null, resolved_at: null, created_at: iso(3) },
  { id: 'n-3', target_kind: 'contact', target_id: 'c-lumi', author_id: ME, body: '담당자 연락은 오후 2시 이후 선호.', mentions: [], attachments: [], resolved: true, resolved_by: ME, resolved_at: iso(30), created_at: iso(40) },
];
for (const n of notes) { const t = tasks.find(x => x.id === n.target_id); if (t) t.note_count++; }

// ---------- 예외 ----------
const exceptions: WorkException[] = [
  { id: 'ex-1', kind: 'QC', severity: 'HIGH', status: 'OPEN', title: '여름 끝에서 — 트랙 2 클리핑 감지', detail: 'True peak +0.4 dBTP, 3구간에서 클리핑. 아티스트 보완 요청 또는 예외 승인 필요.', source: 'qc-worker', related: [R('r3c4d5e6', '여름 끝에서')], assignee_id: null, task_id: null, occurrences: 1, first_seen: iso(1.5), last_seen: iso(1.5), resolved_at: null, resolution: null },
  { id: 'ex-2', kind: 'DSP_REJECTION', severity: 'HIGH', status: 'ASSIGNED', title: 'Afterglow — Spotify 반려 (Featured artist in title)', detail: '곡명에 feat. 표기. 참여 아티스트 필드로 옮겨 재전송 필요.', source: 'dsp-ack-ingest', related: [R('r9', 'Afterglow'), { kind: 'dsp', id: 'D-5', label: 'Spotify' }], assignee_id: TAE, task_id: tasks[3].id, occurrences: 2, first_seen: iso(52), last_seen: iso(8), resolved_at: null, resolution: null },
  { id: 'ex-3', kind: 'RIGHTS_DOCS', severity: 'MEDIUM', status: 'OPEN', title: '여름 끝에서 — 공동 작곡 권리 확인서 서명 누락', detail: '서명 페이지가 빠진 PDF. 보완 요청 후 7일 경과.', source: 'rights-checker', related: [R('r3c4d5e6', '여름 끝에서'), { kind: 'document', id: 'doc-3', label: '공동 작곡 권리 확인서' }], assignee_id: null, task_id: null, occurrences: 1, first_seen: iso(30), last_seen: iso(30), resolved_at: null, resolution: null },
  { id: 'ex-4', kind: 'METADATA_CONFLICT', severity: 'MEDIUM', status: 'OPEN', title: 'Blue Hour — ISRC가 다른 발매와 겹쳐요', detail: 'KR-A12-26-00031이 Nova Sound의 2025년 싱글과 중복. 재발매인지 확인 필요.', source: 'identifier-check', related: [R('r2b3c4d5', 'Blue Hour')], assignee_id: null, task_id: null, occurrences: 1, first_seen: iso(4), last_seen: iso(4), resolved_at: null, resolution: null },
  { id: 'ex-5', kind: 'SETTLEMENT_MISMATCH', severity: 'CRITICAL', status: 'ASSIGNED', title: 'Apple Music 9월 리포트 — 합계 ₩412,300 차이', detail: '리포트 합계와 입금액 차이 3.1%. 환율 적용일 차이 또는 누락 행 의심.', source: 'settlement-reconciler', related: [{ kind: 'dsp', id: 'D-6', label: 'Apple Music' }], assignee_id: ME, task_id: tasks[4].id, occurrences: 1, first_seen: iso(6), last_seen: iso(6), resolved_at: null, resolution: null },
  { id: 'ex-6', kind: 'DELIVERY_FAILURE', severity: 'CRITICAL', status: 'OPEN', title: 'FLO SFTP 전송 실패 — 연결 시간 초과 (3회)', detail: 'sftp.flo.example:22 연결 시간 초과. 자동 재시도 3회 모두 실패.', source: 'delivery-runner', related: [{ kind: 'dsp', id: 'D-3', label: 'FLO' }, R('r4d5e6f7', 'Paper Moon')], assignee_id: null, task_id: null, occurrences: 3, first_seen: iso(2), last_seen: iso(0.3), resolved_at: null, resolution: null },
  { id: 'ex-7', kind: 'QC', severity: 'LOW', status: 'RESOLVED', title: 'Midnight Drive — 앞 무음 2.1초', detail: '의도된 인트로로 확인.', source: 'qc-worker', related: [R('r6f7a8b9', 'Midnight Drive')], assignee_id: KIM, task_id: null, occurrences: 1, first_seen: iso(80), last_seen: iso(80), resolved_at: iso(70), resolution: '의도된 무음 — 아티스트 확인' },
];

// ---------- 거래처 ----------
const contacts: Contact[] = [
  { id: 'c-lumi', kind: 'LABEL', name: 'Lumi Records', company: 'Lumi Records', person: '정하늘 대표', email: 'ceo@lumi.example', phone: '010-2345-6789', owner_id: KIM, last_contact_at: iso(26), next_follow_up_at: iso(-4), contract_stage: 'RENEWAL', tags: ['주요 레이블'], org_id: 'org-r4', dsp_code: null, memo: '연 12장 발매. 갱신 협의 중.', created_at: iso(2000), updated_at: iso(26) },
  { id: 'c-nova', kind: 'LABEL', name: 'Nova Sound', company: 'Nova Sound', person: '이준호 A&R', email: 'ar@nova.example', phone: '010-9876-5432', owner_id: ME, last_contact_at: iso(100), next_follow_up_at: iso(20), contract_stage: 'SIGNED', tags: [], org_id: 'org-y', dsp_code: null, memo: '', created_at: iso(900), updated_at: iso(100) },
  { id: 'c-hangyeol', kind: 'ARTIST', name: '한결', company: '한결 뮤직', person: '한결', email: 'hangyeol@example.com', phone: '', owner_id: PARK, last_contact_at: iso(20), next_follow_up_at: iso(-30), contract_stage: 'SIGNED', tags: ['신인'], org_id: 'org-r1a2b3c4', dsp_code: null, memo: '10/17 발매 희망.', created_at: iso(300), updated_at: iso(20) },
  { id: 'c-spotify', kind: 'DSP', name: 'Spotify', company: 'Spotify AB', person: 'Content Ops', email: 'content-ops@spotify.example', phone: '', owner_id: TAE, last_contact_at: iso(52), next_follow_up_at: iso(-2), contract_stage: 'SIGNED', tags: ['해외'], org_id: null, dsp_code: 'D-5', memo: '반려 문의는 티켓 포털로.', created_at: iso(5000), updated_at: iso(52) },
  { id: 'c-melon', kind: 'DSP', name: '멜론', company: '카카오엔터테인먼트', person: '유통 제휴팀', email: 'partner@melon.example', phone: '02-000-0000', owner_id: TAE, last_contact_at: iso(200), next_follow_up_at: iso(-200), contract_stage: 'RENEWAL', tags: ['국내'], org_id: null, dsp_code: 'D-1', memo: '연말 계약 갱신.', created_at: iso(6000), updated_at: iso(200) },
  { id: 'c-pub', kind: 'PARTNER', name: 'Sori Publishing', company: 'Sori Publishing', person: '최유진 매니저', email: 'yj@sori.example', phone: '010-1111-2222', owner_id: ME, last_contact_at: null, next_follow_up_at: iso(-48), contract_stage: 'NEGOTIATING', tags: ['퍼블리싱'], org_id: null, dsp_code: null, memo: '퍼블리싱 대행 제휴 제안.', created_at: iso(60), updated_at: iso(60) },
];
const activities: Record<string, ContactActivity[]> = {
  'c-lumi': [
    { id: 'a1', kind: 'CALL', summary: '갱신 조건 1차 통화 — 7% 요청, 최소 지급액 낮춰 달라는 의견', by: KIM, at: iso(26) },
    { id: 'a2', kind: 'EMAIL', summary: '현재 계약서 사본과 조건표 발송', by: KIM, at: iso(120) },
  ],
  'c-spotify': [{ id: 'a3', kind: 'EMAIL', summary: 'Afterglow 반려 사유 문의 티켓 등록 (#88123)', by: TAE, at: iso(52) }],
};

// ---------- DSP 운영 ----------
function dsp(code: string, v: Partial<DspOps>): DspOps {
  return {
    code, name: DSP_NAME[code] ?? code, region: ['D-1', 'D-2', 'D-3', 'D-4'].includes(code) ? 'KR' : 'GLOBAL',
    contract_stage: 'SIGNED', contract_renewal_at: null, integration: 'DDEX_SFTP', health: 'HEALTHY', last_sync_at: iso(0.4),
    owner_id: TAE, sla_hours: 24, deliveries_30d: 120, failures_30d: 1, failure_trend: [1, 0.8, 1.2, 0.6, 0.9, 1.1, 0.7, 0.8],
    avg_approval_hours: 20, rejections: [], open_inquiries: 0, recent_inquiries: [], sla_breaches_30d: 0, ...v,
  };
}
const dsps: DspOps[] = [
  dsp('D-1', { contract_stage: 'RENEWAL', contract_renewal_at: new Date(now + 70 * 24 * H).toISOString(), deliveries_30d: 184, failures_30d: 3, avg_approval_hours: 6.5, rejections: [{ reason: '앨범 커버 텍스트', count: 2 }, { reason: '장르 코드', count: 1 }], open_inquiries: 1, recent_inquiries: [{ id: 'dq-1', subject: 'Paper Moon 노출 지연', at: iso(3), status: 'OPEN' }], failure_trend: [1.2, 1.5, 1.1, 1.8, 1.3, 1.6, 1.4, 1.6] }),
  dsp('D-2', { deliveries_30d: 162, failures_30d: 2, avg_approval_hours: 9, rejections: [{ reason: '메타데이터 표기', count: 2 }] }),
  dsp('D-3', { health: 'DOWN', last_sync_at: iso(2.1), deliveries_30d: 140, failures_30d: 11, avg_approval_hours: 14, rejections: [{ reason: '전송 시간 초과', count: 8 }, { reason: '파일 손상', count: 1 }], open_inquiries: 1, recent_inquiries: [{ id: 'dq-2', subject: 'SFTP 접속 불가 문의', at: iso(1), status: 'OPEN' }], sla_breaches_30d: 2, failure_trend: [0.8, 0.9, 1.0, 1.4, 2.2, 3.1, 5.0, 7.9] }),
  dsp('D-5', { integration: 'DDEX_API', health: 'DEGRADED', deliveries_30d: 210, failures_30d: 9, avg_approval_hours: 41, sla_hours: 48, rejections: [{ reason: 'Featured artist in title', count: 4 }, { reason: 'Generic artist name', count: 3 }, { reason: 'Artwork quality', count: 2 }], open_inquiries: 2, recent_inquiries: [{ id: 'dq-3', subject: 'Afterglow 반려 사유', at: iso(52), status: 'OPEN' }, { id: 'dq-4', subject: '아티스트 프로필 병합 요청', at: iso(80), status: 'ANSWERED' }], sla_breaches_30d: 1, failure_trend: [3.1, 2.8, 3.5, 4.0, 3.6, 4.2, 4.4, 4.3] }),
  dsp('D-6', { integration: 'DDEX_SFTP', deliveries_30d: 198, failures_30d: 2, avg_approval_hours: 30, sla_hours: 48, rejections: [{ reason: 'Explicit 표기 누락', count: 2 }] }),
  dsp('D-7', { integration: 'DDEX_API', deliveries_30d: 205, failures_30d: 1, avg_approval_hours: 12, rejections: [{ reason: 'Content ID 충돌', count: 1 }] }),
  dsp('D-9', { contract_stage: 'NEGOTIATING', health: 'ONBOARDING', integration: 'DDEX_SFTP', last_sync_at: null, deliveries_30d: 0, failures_30d: 0, avg_approval_hours: null, failure_trend: [0, 0, 0, 0, 0, 0, 0, 0], owner_id: ME }),
];

const uploads = new Map<string, { id: string; name: string; size: number; content_type: string }>();

// ---------- 업무함 읽음 ----------
const readIds = new Set<string>();

function buildInbox(): InboxItem[] {
  const out: InboxItem[] = [];
  const pr = (p: Priority) => p;
  for (const t of tasks.map(refresh)) {
    if (t.status === 'DONE' || t.status === 'CANCELLED') continue;
    const due = t.due_at ?? t.sla_due_at;
    const ref: EntityRef = { kind: 'task', id: t.id, label: t.key };
    if (t.assignee_id === ME && t.status !== 'IN_REVIEW') {
      out.push({ id: `as:${t.id}`, kind: 'ASSIGNED', title: t.title, summary: `${t.key} · 나에게 배정`, priority: pr(t.priority), due_at: due, at: t.assigned_at ?? t.created_at, link: `/admin/tasks/${t.id}`, read: false, ref });
    }
    if (t.reviewer_id === ME && t.status === 'IN_REVIEW') {
      out.push({ id: `rv:${t.id}`, kind: 'REVIEW_REQUEST', title: t.title, summary: `${t.key} · 검토 승인 요청`, priority: pr(t.priority), due_at: due, at: t.updated_at, link: `/admin/tasks/${t.id}`, read: false, ref });
    }
    if (t.assignee_id === ME && (events[t.id] ?? []).some(e => e.action === 'CHANGES_REQUESTED') && t.status === 'IN_PROGRESS') {
      out.push({ id: `cr:${t.id}`, kind: 'CORRECTION', title: t.title, summary: `${t.key} · 검토자가 보완을 요청했어요`, priority: pr(t.priority), due_at: due, at: t.updated_at, link: `/admin/tasks/${t.id}`, read: false, ref });
    }
    if (t.sla_state === 'OVERDUE' && (t.assignee_id === ME || people.find(p => p.user_id === ME)?.role === policyOf(t.type)?.escalate_to)) {
      out.push({ id: `sla:${t.id}`, kind: 'SLA_BREACH', title: t.title, summary: `${t.key} · SLA 기한을 넘겼어요`, priority: 'URGENT', due_at: due, at: due ?? t.created_at, link: `/admin/tasks/${t.id}`, read: false, ref });
    }
  }
  for (const n of notes) {
    if (!n.mentions.includes(ME) || n.resolved) continue;
    const link = n.target_kind === 'task' ? `/admin/tasks/${n.target_id}` : n.target_kind === 'release' ? `/admin/reviews/${n.target_id}` : n.target_kind === 'contact' ? `/admin/crm/${n.target_id}` : '/admin/inbox';
    const who = people.find(p => p.user_id === n.author_id)?.name ?? '동료';
    out.push({ id: `mn:${n.id}`, kind: 'MENTION', title: `${who}님이 나를 언급했어요`, summary: n.body.slice(0, 90), priority: 'NORMAL', due_at: null, at: n.created_at, link, read: false, ref: { kind: n.target_kind, id: n.target_id, label: '' } });
  }
  for (const x of exceptions) {
    if (x.status === 'RESOLVED' || x.status === 'IGNORED') continue;
    if (x.assignee_id && x.assignee_id !== ME) continue;
    const kind: InboxKind = x.kind === 'RIGHTS_DOCS' ? 'RIGHTS' : 'AUTOMATION_FAILED';
    out.push({ id: `ex:${x.id}`, kind, title: x.title, summary: `${x.source} · ${x.occurrences > 1 ? `${x.occurrences}회 반복` : '1회'}`, priority: x.severity === 'CRITICAL' ? 'URGENT' : x.severity === 'HIGH' ? 'HIGH' : 'NORMAL', due_at: null, at: x.last_seen, link: `/admin/exceptions?id=${x.id}`, read: false, ref: { kind: 'exception', id: x.id, label: x.title } });
  }
  out.push(
    { id: 'iq:iq-1', kind: 'INQUIRY', title: '심사가 얼마나 걸리나요?', summary: '한결 뮤직 · 발매·심사 · 20시간째 답변 대기', priority: 'HIGH', due_at: iso(-4), at: iso(20), link: '/admin/inquiries/iq-1', read: false, ref: { kind: 'inquiry', id: 'iq-1', label: '심사가 얼마나 걸리나요?' } },
    { id: 'iq:iq-2', kind: 'INQUIRY', title: '지급 계좌 변경 문의', summary: 'Nova Sound · 정산·지급 · 7시간째 답변 대기', priority: 'NORMAL', due_at: iso(-10), at: iso(7), link: '/admin/inquiries/iq-2', read: false, ref: { kind: 'inquiry', id: 'iq-2', label: '지급 계좌 변경 문의' } },
    { id: 'rt:r5e6f7a8', kind: 'RIGHTS', title: '고요한 밤의 노래 — 커버 무단 사용 신고', summary: '권리 보류 중 · 신고자 증빙 대기', priority: 'HIGH', due_at: null, at: iso(70), link: '/admin/reviews/r5e6f7a8', read: false, ref: R('r5e6f7a8', '고요한 밤의 노래') },
  );
  for (const i of out) i.read = readIds.has(i.id);
  return out.sort((a, b) => Number(a.read) - Number(b.read) || rank(b.priority) - rank(a.priority) || b.at.localeCompare(a.at));
}
const rank = (p: Priority) => ['LOW', 'NORMAL', 'HIGH', 'URGENT'].indexOf(p);

function event(t: WorkTask, action: string, detail: string | null = null) {
  (events[t.id] ??= []).push({ at: new Date().toISOString(), actor_id: ME, action, detail });
}

const nextRecurrence = (r: NonNullable<WorkTask['recurrence']>, from: number) => {
  const d = new Date(from);
  if (r.freq === 'DAILY') d.setDate(d.getDate() + r.interval);
  else if (r.freq === 'WEEKLY') d.setDate(d.getDate() + 7 * r.interval);
  else d.setMonth(d.getMonth() + r.interval);
  return d.toISOString();
};

function matchTask(t: WorkTask, q: TaskQuery) {
  const view = q.view ?? 'mine';
  if (view === 'mine' && t.assignee_id !== ME) return false;
  if (view === 'review' && t.reviewer_id !== ME) return false;
  if (view === 'created' && t.created_by !== ME) return false;
  const st = q.status ?? 'open';
  const closed = t.status === 'DONE' || t.status === 'CANCELLED';
  if (st === 'open' && closed) return false;
  if (st === 'done' && !closed) return false;
  if (q.type && t.type !== q.type) return false;
  if (q.assignee && t.assignee_id !== q.assignee) return false;
  if (q.related_kind && !t.related.some(r => r.kind === q.related_kind && r.id === q.related_id)) return false;
  const term = q.q?.trim().toLowerCase();
  if (term && ![t.title, t.key, t.description, ...t.tags].some(v => v.toLowerCase().includes(term))) return false;
  return true;
}

export const mockWork: WorkApi = {
  summary: () => {
    const inbox = buildInbox();
    const mine = tasks.filter(t => t.assignee_id === ME && t.status !== 'DONE' && t.status !== 'CANCELLED');
    const by_kind: WorkSummary['by_kind'] = {};
    for (const i of inbox) if (!i.read) by_kind[i.kind] = (by_kind[i.kind] ?? 0) + 1;
    return wait<WorkSummary>({
      inbox_unread: inbox.filter(i => !i.read).length, inbox_total: inbox.length, my_open: mine.length,
      due_soon: mine.filter(t => t.sla_state === 'DUE_SOON').length,
      overdue: tasks.filter(t => t.sla_state === 'OVERDUE').length,
      exceptions_open: exceptions.filter(x => x.status === 'OPEN' || x.status === 'ASSIGNED').length,
      follow_ups_due: contacts.filter(c => c.next_follow_up_at && Date.parse(c.next_follow_up_at) <= Date.now() + 24 * H).length,
      by_kind,
    });
  },
  people: () => wait({ items: people }),

  inbox: (kind?: InboxKind | '', unread?: boolean) => wait({ items: buildInbox().filter(i => (!kind || i.kind === kind) && (!unread || !i.read)) }),
  readInbox: (ids: string[]) => { ids.forEach(i => readIds.add(i)); return wait({ ok: true as const }); },

  tasks: (q: TaskQuery = {}) => wait({
    items: tasks.map(refresh).filter(t => matchTask(t, q)).sort((a, b) =>
      (a.status === 'DONE' ? 1 : 0) - (b.status === 'DONE' ? 1 : 0) || rank(b.priority) - rank(a.priority) ||
      String(a.due_at ?? a.sla_due_at ?? '9').localeCompare(String(b.due_at ?? b.sla_due_at ?? '9'))),
  }),
  task: (id: string) => {
    const t = tasks.find(x => x.id === id) ?? fail('NOT_FOUND', 404);
    return wait({ task: refresh(t), events: [...(events[id] ?? [])].reverse() });
  },
  createTask: (body: TaskInput) => {
    if (!body.title.trim()) fail('VALIDATION');
    const at = new Date().toISOString();
    const t = mk(0, {
      ...body, title: body.title.trim(), description: body.description ?? '', assignee_id: body.assignee_id ?? null, reviewer_id: body.reviewer_id ?? null,
      due_at: body.due_at ?? null, related: body.related ?? [], tags: body.tags ?? [], created_by: ME, created_at: at, updated_at: at,
      assigned_at: body.assignee_id ? at : null,
      recurrence: body.recurrence ? { ...body.recurrence, next_at: nextRecurrence(body.recurrence, Date.now()) } : null,
    });
    events[t.id] = [{ at, actor_id: ME, action: 'CREATED', detail: null }];
    return wait(t);
  },
  updateTask: (id: string, patch: TaskPatch) => {
    const t = tasks.find(x => x.id === id) ?? fail('NOT_FOUND', 404);
    if (patch.row_version !== t.row_version) fail('CONFLICT', 409);
    const at = new Date().toISOString();
    if (patch.review) {
      if (t.status !== 'IN_REVIEW' || t.reviewer_id !== ME) fail('FORBIDDEN', 403);
      if (patch.review === 'APPROVE') { t.status = 'DONE'; t.completed_at = at; event(t, 'APPROVED', patch.review_note ?? null); }
      else { t.status = 'IN_PROGRESS'; event(t, 'CHANGES_REQUESTED', patch.review_note ?? null); }
    }
    if (patch.status && patch.status !== t.status) {
      let next = patch.status;
      // 검토자가 있으면 완료 대신 검토 요청
      if (next === 'DONE' && t.reviewer_id && t.status !== 'IN_REVIEW' && t.reviewer_id !== ME) next = 'IN_REVIEW';
      t.status = next;
      if (next === 'IN_PROGRESS') t.started_at ??= at;
      if (next === 'DONE') {
        t.completed_at = at;
        if (t.recurrence) {
          const r = t.recurrence;
          const n = mk(0, { ...structuredClone(t), id: `t-${uid()}`, key: `WRK-${seq}`, status: 'TODO', started_at: null, completed_at: null, created_at: at, updated_at: at, assigned_at: at, note_count: 0, row_version: 1, priority_raised_from: null, recurrence: { ...r, next_at: nextRecurrence(r, Date.now()) } });
          n.sla_due_at = slaDueAt(at, policyOf(n.type));
          event(t, 'RECURRED', n.key);
        }
      }
      event(t, next === 'IN_REVIEW' ? 'REVIEW_REQUESTED' : 'STATUS', next);
    }
    const fields: (keyof TaskInput)[] = ['title', 'description', 'type', 'priority', 'assignee_id', 'reviewer_id', 'due_at', 'recurrence', 'related', 'tags'];
    for (const f of fields) {
      if (patch[f] === undefined) continue;
      if (JSON.stringify(patch[f]) === JSON.stringify(t[f])) continue;
      (t as unknown as Record<string, unknown>)[f] = patch[f];
      if (f === 'assignee_id') { t.assigned_at = at; event(t, 'ASSIGNED', people.find(p => p.user_id === patch.assignee_id)?.name ?? null); }
      else if (f === 'priority') { t.priority_raised_from = null; event(t, 'PRIORITY', String(patch.priority)); }
      else if (f === 'due_at') event(t, 'DUE', patch.due_at ?? null);
      else if (f === 'type') t.sla_due_at = slaDueAt(t.created_at, policyOf(t.type));
      else if (f === 'title' || f === 'description') event(t, 'EDITED');
    }
    t.updated_at = at;
    t.row_version++;
    return wait(refresh(t));
  },

  notes: (kind: EntityKind, targetId: string) => wait({ items: notes.filter(n => n.target_kind === kind && n.target_id === targetId).sort((a, b) => a.created_at.localeCompare(b.created_at)) }),
  addNote: (body: NoteInput) => {
    if (!body.body.trim() && body.attachments.length === 0) fail('VALIDATION');
    const n: WorkNote = {
      id: `n-${uid()}`, target_kind: body.target_kind, target_id: body.target_id, author_id: ME, body: body.body.trim(), mentions: body.mentions,
      attachments: body.attachments.map(u => uploads.get(u) ?? fail('NOT_FOUND', 404)),
      resolved: false, resolved_by: null, resolved_at: null, created_at: new Date().toISOString(),
    };
    notes.push(n);
    const t = body.target_kind === 'task' ? tasks.find(x => x.id === body.target_id) : null;
    if (t) { t.note_count++; event(t, 'NOTE'); }
    return wait(n);
  },
  resolveNote: (id: string, resolved: boolean) => {
    const n = notes.find(x => x.id === id) ?? fail('NOT_FOUND', 404);
    n.resolved = resolved; n.resolved_by = resolved ? ME : null; n.resolved_at = resolved ? new Date().toISOString() : null;
    return wait(n);
  },
  upload: async (file: File) => {
    const upId = `att-${uid()}`;
    uploads.set(upId, { id: upId, name: file.name, size: file.size, content_type: file.type || 'application/octet-stream' });
    return wait(upId);
  },
  attachmentUrl: () => '',

  exceptions: (kind?: ExceptionKind | '', status?: ExceptionStatus | 'active' | '') => wait({
    items: exceptions
      .filter(x => (!kind || x.kind === kind) && (!status || (status === 'active' ? x.status === 'OPEN' || x.status === 'ASSIGNED' : x.status === status)))
      .sort((a, b) => ['LOW', 'MEDIUM', 'HIGH', 'CRITICAL'].indexOf(b.severity) - ['LOW', 'MEDIUM', 'HIGH', 'CRITICAL'].indexOf(a.severity) || b.last_seen.localeCompare(a.last_seen)),
  }),
  actException: (id: string, a: ExceptionAction) => {
    const x = exceptions.find(e => e.id === id) ?? fail('NOT_FOUND', 404);
    const at = new Date().toISOString();
    if (a.action === 'ASSIGN') { x.assignee_id = a.assignee_id; x.status = 'ASSIGNED'; }
    else if (a.action === 'RESOLVE' || a.action === 'IGNORE') { x.status = a.action === 'RESOLVE' ? 'RESOLVED' : 'IGNORED'; x.resolution = a.resolution; x.resolved_at = at; }
    else if (a.action === 'REOPEN') { x.status = x.assignee_id ? 'ASSIGNED' : 'OPEN'; x.resolution = null; x.resolved_at = null; }
    else if (a.action === 'TO_TASK') {
      if (x.task_id) fail('CONFLICT', 409);
      const type: TaskType = x.kind === 'RIGHTS_DOCS' ? 'RIGHTS' : x.kind === 'SETTLEMENT_MISMATCH' ? 'SETTLEMENT' : x.kind === 'DSP_REJECTION' ? 'DSP_INQUIRY' : x.kind === 'DELIVERY_FAILURE' ? 'URGENT_DELIVERY' : 'RELEASE';
      const t = mk(0, { title: x.title, description: x.detail, type, priority: a.priority ?? (x.severity === 'CRITICAL' ? 'URGENT' : x.severity === 'HIGH' ? 'HIGH' : 'NORMAL'), assignee_id: a.assignee_id ?? ME, created_by: ME, created_at: at, updated_at: at, related: [...x.related, { kind: 'exception', id: x.id, label: x.title }] });
      t.sla_due_at = slaDueAt(at, policyOf(type));
      x.task_id = t.id; x.assignee_id = t.assignee_id; x.status = 'ASSIGNED';
    }
    return wait(x);
  },

  sla: () => wait({ items: policies }),
  saveSla: (p: SlaPolicy) => {
    if (p.target_minutes < 15) fail('VALIDATION');
    policies = policies.map(x => (x.type === p.type ? p : x));
    for (const t of tasks) if (t.type === p.type && t.status !== 'DONE') { t.sla_due_at = slaDueAt(t.created_at, p); refresh(t); }
    return wait(p);
  },

  contacts: (kind?: ContactKind | '', q?: string, due?: boolean) => {
    const term = q?.trim().toLowerCase();
    return wait({
      items: contacts
        .filter(c => (!kind || c.kind === kind) && (!term || [c.name, c.company, c.person, c.email, ...c.tags].some(v => v.toLowerCase().includes(term))))
        .filter(c => !due || (c.next_follow_up_at && Date.parse(c.next_follow_up_at) <= Date.now() + 24 * H))
        .sort((a, b) => String(a.next_follow_up_at ?? '9').localeCompare(String(b.next_follow_up_at ?? '9'))),
    });
  },
  contact: (id: string) => {
    const c = contacts.find(x => x.id === id) ?? fail('NOT_FOUND', 404);
    const d: ContactDetail = { contact: c, activities: [...(activities[id] ?? [])].sort((a, b) => b.at.localeCompare(a.at)), tasks: tasks.filter(t => t.related.some(r => r.kind === 'contact' && r.id === id) || (c.dsp_code != null && t.related.some(r => r.kind === 'dsp' && r.id === c.dsp_code))) };
    return wait(d);
  },
  createContact: (body: ContactInput) => {
    if (!body.name.trim()) fail('VALIDATION');
    const at = new Date().toISOString();
    const c: Contact = { ...body, id: `c-${uid()}`, last_contact_at: null, org_id: null, dsp_code: null, created_at: at, updated_at: at };
    contacts.push(c);
    return wait(c);
  },
  updateContact: (id: string, body: Partial<ContactInput>) => {
    const c = contacts.find(x => x.id === id) ?? fail('NOT_FOUND', 404);
    Object.assign(c, body, { updated_at: new Date().toISOString() });
    return wait(c);
  },
  addActivity: (id: string, body: { kind: ActivityKind; summary: string; next_follow_up_at?: string | null }) => {
    const c = contacts.find(x => x.id === id) ?? fail('NOT_FOUND', 404);
    const a: ContactActivity = { id: uid(), kind: body.kind, summary: body.summary, by: ME, at: new Date().toISOString() };
    (activities[id] ??= []).push(a);
    if (body.kind !== 'NOTE') c.last_contact_at = a.at;
    if (body.next_follow_up_at !== undefined) c.next_follow_up_at = body.next_follow_up_at;
    return wait(a);
  },

  dspOps: () => wait({ items: dsps }),
  updateDspOps: (code: string, body: DspOpsPatch) => {
    const d = dsps.find(x => x.code === code) ?? fail('NOT_FOUND', 404);
    Object.assign(d, body);
    return wait(d);
  },

  analytics: (days: number) => {
    const daily = Array.from({ length: days }, (_, i) => {
      const d = new Date(now - (days - 1 - i) * 24 * H);
      const wk = d.getDay() === 0 || d.getDay() === 6;
      const wave = Math.sin(i / 2.3) * 4;
      const manual = Math.max(0, Math.round((wk ? 5 : 22) + wave + ((i * 7) % 5)));
      const auto = Math.max(0, Math.round((wk ? 30 : 64) + wave * 2 + ((i * 11) % 9)));
      return { date: d.toISOString().slice(0, 10), manual, auto, created: Math.round(manual * 1.05 + ((i * 3) % 4)) };
    });
    const a: Analytics = {
      days, daily,
      people: [
        { user_id: ME, done: 7 * days / 7 * 6 | 0, avg_minutes: 142, open: tasks.filter(t => t.assignee_id === ME && t.status !== 'DONE').length, overdue: 1 },
        { user_id: KIM, done: 5 * days / 7 * 6 | 0, avg_minutes: 196, open: 3, overdue: 0 },
        { user_id: TAE, done: 6 * days / 7 * 5 | 0, avg_minutes: 88, open: 4, overdue: 1 },
        { user_id: PARK, done: 9 * days / 7 * 5 | 0, avg_minutes: 41, open: 2, overdue: 0 },
      ],
      avg_cycle_minutes: 127,
      stages: [
        { key: 'stage1', label: '1차 자동 검사', avg_hours: 0.4, p90_hours: 1.1, wip: 6 },
        { key: 'stage2', label: '2차 검사', avg_hours: 2.2, p90_hours: 6, wip: 4 },
        { key: 'review', label: '담당자 심사', avg_hours: 19.5, p90_hours: 44, wip: 11 },
        { key: 'correction', label: '아티스트 보완', avg_hours: 31, p90_hours: 96, wip: 5 },
        { key: 'second', label: '2차 승인', avg_hours: 6.1, p90_hours: 15, wip: 1 },
        { key: 'sign', label: '계약 서명', avg_hours: 9.8, p90_hours: 30, wip: 3 },
        { key: 'delivery', label: 'DSP 전송·승인', avg_hours: 22, p90_hours: 52, wip: 14 },
      ],
      reject_rate: 0.083, auto_rate: daily.reduce((s, d) => s + d.auto, 0) / daily.reduce((s, d) => s + d.auto + d.manual, 0), sla_met_rate: 0.91,
      dsp_failure: dsps.filter(d => d.deliveries_30d > 0).map(d => ({ code: d.code, name: d.name, rate: d.failures_30d / d.deliveries_30d, failures: d.failures_30d })).sort((x, y) => y.rate - x.rate),
      by_type: (['RELEASE', 'RIGHTS', 'SUPPORT', 'DSP_INQUIRY', 'URGENT_DELIVERY', 'CONTRACT', 'SETTLEMENT', 'LEGAL', 'GENERAL'] as TaskType[]).map((type, i) => ({ type, count: [48, 21, 37, 14, 6, 5, 8, 2, 11][i] })),
    };
    return wait(a);
  },
};
