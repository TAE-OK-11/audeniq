// 체험(목) 빌드용 업무 API — 서버 없이 업무 화면을 확인할 수 있게 메모리 예시 데이터로 동작한다.
// 규칙(하위 업무·검토·반복·보관·메모 수정 이력)은 crates/core/src/work와 같게 흉내 낸다.
import { ApiError, messageForCode } from '../../api/errors';
import { DSP_NAME } from '../dspReqs';
import type {
  Channel, EntityKind, EntityRef, LinkInput, NoteInput, NoteRevision, StaffPerson, TaskDetail, TaskEvent, TaskInput, TaskPatch,
  TaskQuery, WorkNote, WorkTask,
} from './types';
import type { WorkApi } from './api';
import { makeOps, mockExceptions } from './mockOps';
import { makeCrm } from './mockCrm';
import { makeBoards } from './mockBoards';
import { makeDocs } from './mockDocs';

const ME = 'staff-me-0001';
const KIM = 'staff-kim-0002';
const TAE = 'staff-tae-0003';
const PARK = 'staff-park-0004';
const people: StaffPerson[] = [
  { user_id: ME, email: 'staff@audeniq.com', name: 'staff', role: 'ADMIN' },
  { user_id: KIM, email: 'kim@audeniq.com', name: 'kim', role: 'REVIEWER' },
  { user_id: TAE, email: 'taeyeong@audeniq.com', name: 'taeyeong', role: 'OPERATOR' },
  { user_id: PARK, email: 'park@audeniq.com', name: 'park', role: 'SUPPORT' },
];

const now = Date.now();
const H = 3_600_000;
const iso = (hoursAgo: number) => new Date(now - hoursAgo * H).toISOString();
const wait = <T>(v: T) => new Promise<T>(r => setTimeout(() => r(structuredClone(v)), 160));
const fail = (code: string, status = 422): never => { throw new ApiError(messageForCode(code, status), status, code); };
const uid = () => Math.random().toString(16).slice(2, 10);

// ---------- 원천 Entity (체험용: 관리자 체험 데이터의 발매·문의·서류와 같은 id) ----------
const SOURCE: Record<string, [string, string][]> = {
  release: [['r1a2b3c4', '새벽의 온도'], ['r2b3c4d5', 'Blue Hour'], ['r3c4d5e6', '여름 끝에서'], ['r4d5e6f7', 'Paper Moon'], ['r5e6f7a8', '고요한 밤의 노래']],
  org: [['org-r4', 'Lumi Records'], ['org-y', 'Nova Sound'], ['org-r1a2b3c4', '한결 뮤직']],
  artist: [['ar-1', '한결'], ['ar-2', 'NOVA'], ['ar-3', 'Lumi']],
  label: [['lb-1', 'Lumi Records'], ['lb-2', 'Nova Sound']],
  dsp: Object.entries(DSP_NAME).slice(0, 12),
  inquiry: [['iq-1', '심사가 얼마나 걸리나요?'], ['iq-2', '지급 계좌 변경 문의']],
  document: [['doc-1', 'Paper Moon · 배급 신청·계약서'], ['doc-2', '샘플 사용 허락서'], ['doc-3', '공동 작곡 권리 확인서']],
  payout: [['po-1', 'Lumi Records · 184500 KRW']],
  settlement_report: [['rp-1', 'D-6 2026-09-01~2026-09-30']],
};
const SUB: Record<string, string> = { release: 'Lumi Records', dsp: '', document: 'RIGHTS_PROOF', org: 'LABEL' };

let seq = 1040;
const tasks: WorkTask[] = [];
const events: Record<string, TaskEvent[]> = {};
interface Link { id: string; task: string; kind: EntityKind; entity: string; archived: boolean }
const links: Link[] = [];

// ---------- 팀 대화 채널 (메시지는 메모와 같은 저장소) ----------
const channels: Channel[] = [
  ['ch-all', '전체', '모든 직원 공지와 일반 대화'], ['ch-review', '발매 심사', '심사 기준·판단이 애매한 발매 논의'],
  ['ch-dist', '배급', 'DSP 전송·반려·장애 공유'], ['ch-support', '고객 지원', '문의 응대 방법과 에스컬레이션'], ['ch-settle', '정산', '리포트 대사·지급 일정'],
].map(([id, name, topic]) => ({ id, name, topic, created_by: null, created_at: iso(800), archived_at: null }));
const channelReads: Record<string, string> = { 'ch-all': iso(0.5) };

function resolve(kind: EntityKind, id: string): EntityRef {
  if (kind === 'channel') {
    const c = channels.find(x => x.id === id);
    return c ? { kind, id, label: `#${c.name}`, sub: c.topic } : { kind, id, label: null, sub: null, missing: true };
  }
  if (kind === 'task') {
    const t = tasks.find(x => x.id === id);
    return t ? { kind, id, label: `${t.key} ${t.title}`, sub: t.status } : { kind, id, label: null, sub: null, missing: true };
  }
  const hit = (SOURCE[kind] ?? []).find(([k]) => k === id);
  return hit ? { kind, id, label: hit[1], sub: SUB[kind] ?? null } : { kind, id, label: null, sub: null, missing: true };
}
const linksOf = (task: string): EntityRef[] =>
  links.filter(l => l.task === task && !l.archived).map(l => ({ ...resolve(l.kind, l.entity), link_id: l.id }));

function counts(t: WorkTask) {
  const subs = tasks.filter(x => x.parent_id === t.id && !x.archived_at);
  t.subtasks = { total: subs.length, done: subs.filter(x => x.status === 'DONE' || x.status === 'CANCELLED').length };
  t.note_count = notes.filter(n => n.target_kind === 'task' && n.target_id === t.id && !n.archived_at).length;
  t.overdue = !!t.due_at && Date.parse(t.due_at) < Date.now() && t.status !== 'DONE' && t.status !== 'CANCELLED';
  t.links = linksOf(t.id);
  return t;
}

function ev(task: string, action: string, detail: string | null = null, actor: string | null = ME, at = new Date().toISOString()) {
  (events[task] ??= []).push({ at, actor_id: actor, action, detail });
}

function mk(hoursAgo: number, v: Partial<WorkTask> & Pick<WorkTask, 'title' | 'type'>, rel: [EntityKind, string][] = []): WorkTask {
  const created = iso(hoursAgo);
  const t: WorkTask = {
    id: `t-${uid()}`, key: `WRK-${seq++}`, parent_id: null, description: '', status: 'TODO', priority: 'NORMAL',
    assignee_id: ME, reviewer_id: null, due_at: null, overdue: false, changes_requested: false, recurrence: null, recurred_from: null,
    tags: [], created_by: KIM, created_at: created, updated_at: created, assigned_at: created, started_at: null, completed_at: null,
    archived_at: null, row_version: 1, subtasks: { total: 0, done: 0 }, note_count: 0, links: [],
    ...v,
  };
  tasks.push(t);
  ev(t.id, 'CREATED', null, t.created_by, created);
  for (const [kind, entity] of rel) { links.push({ id: `l-${uid()}`, task: t.id, kind, entity, archived: false }); ev(t.id, 'LINKED', `${kind}:${entity}`, t.created_by, created); }
  if (t.status !== 'TODO') ev(t.id, 'STATUS', t.status, t.assignee_id, iso(Math.max(0, hoursAgo - 1)));
  return t;
}

const t1 = mk(1.6, { title: 'Paper Moon 긴급 송출 — 멜론 재전송', type: 'URGENT_DELIVERY', priority: 'URGENT', status: 'IN_PROGRESS', started_at: iso(1.2), due_at: iso(-2), description: '아티스트 공연 일정 때문에 오늘 18시 전 멜론 노출이 필요해요. 전송 실패 원인(ERN 버전) 고친 뒤 재전송.', tags: ['긴급'] }, [['release', 'r4d5e6f7'], ['dsp', 'D-1']]);
mk(1.4, { title: 'ERN 버전 수정본 검증', type: 'URGENT_DELIVERY', parent_id: t1.id, assignee_id: TAE, status: 'DONE', completed_at: iso(1) });
mk(1.4, { title: '멜론 담당자에게 재전송 알림', type: 'URGENT_DELIVERY', parent_id: t1.id, assignee_id: ME });
mk(30, { title: 'Blue Hour 샘플 사용 허락서 권리 검토', type: 'RIGHTS', reviewer_id: KIM, due_at: iso(-20), description: '허락서 원권리자 서명·사용 범위(전 세계, 스트리밍) 확인.' }, [['release', 'r2b3c4d5'], ['document', 'doc-2']]);
mk(20, { title: 'Lumi Records 배급 계약 갱신 검토', type: 'CONTRACT', status: 'IN_REVIEW', reviewer_id: ME, assignee_id: KIM, started_at: iso(18), description: '수수료 8% → 7% 조정 요청. 최소 지급액 조건 변경 여부 검토.' }, [['org', 'org-r4'], ['document', 'doc-1']]);
mk(52, { title: 'Spotify 메타데이터 반려 문의 회신', type: 'DSP_INQUIRY', priority: 'HIGH', assignee_id: TAE, due_at: iso(4) }, [['dsp', 'D-5']]);
mk(6, { title: '9월 정산 리포트 대사 — Apple Music 차액 확인', type: 'SETTLEMENT', tags: ['월말'], due_at: iso(-48) }, [['settlement_report', 'rp-1'], ['dsp', 'D-6']]);
mk(70, { title: '커버 무단 사용 신고 법무 대응', type: 'LEGAL', priority: 'HIGH', status: 'BLOCKED', reviewer_id: KIM, started_at: iso(60), description: '신고자 측 원본 증빙 요청함. 답변 오기 전까지 막힘.' }, [['release', 'r5e6f7a8']]);
mk(3, { title: '주간 DSP 장애 리포트 작성', type: 'GENERAL', priority: 'LOW', assignee_id: TAE, recurrence: { freq: 'WEEKLY', interval: 1, next_at: new Date(now + 4 * 24 * H).toISOString() }, tags: ['반복'] });
mk(26, { title: '새벽의 온도 발매 일정 문의 답변', type: 'SUPPORT', assignee_id: PARK }, [['inquiry', 'iq-1'], ['release', 'r1a2b3c4']]);
mk(96, { title: '신규 아티스트 NOVA 프로필 정리', type: 'ARTIST', status: 'DONE', started_at: iso(90), completed_at: iso(40) }, [['artist', 'ar-2']]);

// ---------- 메모 ----------
const notes: WorkNote[] = [];
const revisions: Record<string, NoteRevision[]> = {};
const attachments = new Map<string, { id: string; name: string; size: number; content_type: string; used: boolean }>();
function note(kind: EntityKind, target: string, author: string, body: string, mentions: string[], hoursAgo: number, extra: Partial<WorkNote> = {}) {
  notes.push({ id: `n-${uid()}`, target_kind: kind, target_id: target, author_id: author, body, mentions, attachments: [], resolved: false, resolved_at: null, resolved_by: null, edited_at: null, revisions: 0, created_at: iso(hoursAgo), archived_at: null, ...extra });
}
note('task', tasks[4].id, KIM, '@staff 수수료 조정 폭은 지난 분기 다른 레이블 조건이랑 맞춰 주세요. 비교표 첨부해요.', [ME], 5, { attachments: [{ id: 'att-1', name: '레이블_수수료_비교.xlsx', size: 48_211, content_type: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet' }] });
note('release', 'r2b3c4d5', TAE, '@staff 이 발매 Spotify에서 피처링 표기 때문에 한 번 반려됐던 이력 있어요. 심사 때 같이 봐 주세요.', [ME], 3);
note('channel', 'ch-all', KIM, '이번 주 금요일 오후 4시에 심사 기준 개편 공유 세션 있어요. 다들 참석 부탁해요!', [], 26);
note('channel', 'ch-all', TAE, '참석할게요. 배급 반려 사례도 몇 개 가져갈게요.', [], 25.5);
note('channel', 'ch-dist', TAE, '멜론 SFTP 연결 거부가 계속돼요. 방화벽 쪽 확인 요청 넣었고, 재전송은 오후에 다시 시도할게요.', [], 4);
note('channel', 'ch-dist', PARK, '@staff Paper Moon 아티스트가 오늘 노출 여부 문의했어요. 예상 시간 공유 가능할까요?', [ME], 1.2);
note('channel', 'ch-dist', TAE, '재전송 큐에 넣었어요. 30분 안에 결과 나올 거예요.', [], 0.4);
note('channel', 'ch-review', KIM, 'AI 생성 커버 정책 질문 — 생성 도구 메타데이터만 있고 사람이 수정한 흔적이 있으면 보완 요청이 맞을까요?', [], 9);
note('org', 'org-r4', ME, '담당자 연락은 오후 2시 이후 선호.', [], 40, { resolved: true, resolved_at: iso(30), resolved_by: ME });

const visible = (t: WorkTask, q: TaskQuery) => {
  const view = q.view ?? 'mine';
  if (view === 'mine' && t.assignee_id !== ME) return false;
  if (view === 'review' && t.reviewer_id !== ME) return false;
  if (view === 'created' && t.created_by !== ME) return false;
  const st = q.status ?? 'open';
  const closed = t.status === 'DONE' || t.status === 'CANCELLED';
  if (st === 'archived') { if (!t.archived_at) return false; } else if (t.archived_at) return false;
  if (st === 'open' && closed) return false;
  if (st === 'done' && !closed) return false;
  if (q.type && t.type !== q.type) return false;
  if (q.priority && t.priority !== q.priority) return false;
  if (q.assignee && t.assignee_id !== q.assignee) return false;
  if (q.due === 'overdue' && !t.overdue) return false;
  if (q.due === 'today' && !(t.due_at && Date.parse(t.due_at) < new Date().setHours(24, 0, 0, 0))) return false;
  if (q.due === 'week' && !(t.due_at && Date.parse(t.due_at) < Date.now() + 7 * 24 * H)) return false;
  if (q.entity_kind && q.entity_id && !links.some(l => l.task === t.id && !l.archived && l.kind === q.entity_kind && l.entity === q.entity_id)) return false;
  if (!(q.subtasks || q.entity_kind) && t.parent_id) return false;
  const term = q.q?.trim().toLowerCase();
  if (term && ![t.title, t.key, t.description, ...t.tags].some(v => v.toLowerCase().includes(term))) return false;
  return true;
};
const rank = (p: string) => ['LOW', 'NORMAL', 'HIGH', 'URGENT'].indexOf(p);

function detail(id: string): TaskDetail {
  const t = tasks.find(x => x.id === id) ?? fail('NOT_FOUND', 404);
  const p = t.parent_id ? tasks.find(x => x.id === t.parent_id) : null;
  return {
    task: counts(t),
    parent: p ? { id: p.id, key: p.key, title: p.title, status: p.status } : null,
    subtasks: tasks.filter(x => x.parent_id === id && !x.archived_at).map(counts),
    events: [...(events[id] ?? [])].reverse(),
  };
}

const nextAt = (freq: string, interval: number, from: number) => {
  const d = new Date(from);
  if (freq === 'DAILY') d.setDate(d.getDate() + interval);
  else if (freq === 'WEEKLY') d.setDate(d.getDate() + 7 * interval);
  else d.setMonth(d.getMonth() + interval);
  return d.toISOString();
};

function addLink(task: string, l: LinkInput) {
  if (resolve(l.entity_kind, l.entity_id).missing) fail('ENTITY_NOT_FOUND', 400);
  if (links.some(x => x.task === task && !x.archived && x.kind === l.entity_kind && x.entity === l.entity_id)) return;
  links.push({ id: `l-${uid()}`, task, kind: l.entity_kind, entity: l.entity_id, archived: false });
  ev(task, 'LINKED', `${l.entity_kind}:${l.entity_id}`);
}

export const mockWork: WorkApi = {
  people: () => wait({ items: people }),
  searchEntities: (kind: EntityKind, q: string) => {
    const term = q.trim().toLowerCase();
    const src: [string, string][] = kind === 'task' ? tasks.filter(t => !t.archived_at).map(t => [t.id, `${t.key} ${t.title}`]) : SOURCE[kind] ?? [];
    return wait({ items: src.filter(([id, label]) => !term || label.toLowerCase().includes(term) || id === q).slice(0, 20).map(([id]) => resolve(kind, id)) });
  },

  tasks: (q: TaskQuery = {}) => wait({
    items: tasks.map(counts).filter(t => visible(t, q)).sort((a, b) =>
      Number(a.status === 'DONE') - Number(b.status === 'DONE') || rank(b.priority) - rank(a.priority) ||
      String(a.due_at ?? '9').localeCompare(String(b.due_at ?? '9'))),
  }),
  task: (id: string) => wait(detail(id)),
  createTask: (body: TaskInput) => {
    if (!body.title.trim()) fail('TITLE_REQUIRED', 400);
    if (body.parent_id) {
      const p = tasks.find(x => x.id === body.parent_id);
      if (!p || p.parent_id || p.archived_at) fail('PARENT_INVALID', 400);
    }
    const at = new Date().toISOString();
    const t = mk(0, {
      title: body.title.trim(), description: body.description ?? '', type: body.type, priority: body.priority, parent_id: body.parent_id ?? null,
      assignee_id: body.assignee_id ?? null, reviewer_id: body.reviewer_id ?? null, due_at: body.due_at ?? null, tags: body.tags ?? [],
      created_by: ME, created_at: at, updated_at: at, assigned_at: body.assignee_id ? at : null,
      recurrence: body.recurrence ? { ...body.recurrence, next_at: nextAt(body.recurrence.freq, body.recurrence.interval, Date.now()) } : null,
    });
    events[t.id] = [{ at, actor_id: ME, action: 'CREATED', detail: null }];
    for (const l of body.links ?? []) addLink(t.id, l);
    if (t.parent_id) ev(t.parent_id, 'SUBTASK_ADDED', t.title);
    return wait(detail(t.id));
  },
  updateTask: (id: string, p: TaskPatch) => {
    const t = tasks.find(x => x.id === id) ?? fail('NOT_FOUND', 404);
    if (t.archived_at) fail('TASK_ARCHIVED');
    if (p.row_version !== t.row_version) fail('CONFLICT', 409);
    counts(t);
    const at = new Date().toISOString();
    let done = false;
    if (p.review) {
      if (t.status !== 'IN_REVIEW') fail('TASK_NOT_IN_REVIEW');
      if (t.reviewer_id !== ME) fail('FORBIDDEN', 403);
      if (p.review === 'APPROVE') {
        if (t.subtasks.total > t.subtasks.done) fail('TASK_SUBTASKS_OPEN');
        t.status = 'DONE'; t.completed_at = at; t.changes_requested = false; done = true; ev(id, 'APPROVED', p.review_note ?? null);
      } else {
        if (!p.review_note?.trim()) fail('REVIEW_NOTE_REQUIRED', 400);
        t.status = 'IN_PROGRESS'; t.changes_requested = true; ev(id, 'CHANGES_REQUESTED', p.review_note ?? null);
      }
    } else if (p.status && p.status !== t.status) {
      let next = p.status;
      if (next === 'DONE' && t.status !== 'IN_REVIEW' && t.reviewer_id && t.reviewer_id !== ME) next = 'IN_REVIEW';
      if (next === 'DONE' && t.subtasks.total > t.subtasks.done) fail('TASK_SUBTASKS_OPEN');
      t.status = next;
      if (next === 'IN_PROGRESS') { t.started_at ??= at; t.completed_at = null; }
      if (next === 'IN_REVIEW') t.changes_requested = false;
      if (next === 'DONE') { t.completed_at = at; done = true; }
      if (next === 'CANCELLED') t.completed_at = at;
      if (next === 'TODO' || next === 'BLOCKED') t.completed_at = null;
      ev(id, next === 'IN_REVIEW' ? 'REVIEW_REQUESTED' : 'STATUS', next);
    }
    const fields = ['title', 'description', 'type', 'priority', 'assignee_id', 'reviewer_id', 'due_at', 'recurrence', 'tags'] as const;
    for (const f of fields) {
      if (p[f] === undefined || JSON.stringify(p[f]) === JSON.stringify(t[f])) continue;
      (t as unknown as Record<string, unknown>)[f] = p[f];
      const action = { title: 'EDITED', description: 'EDITED', type: 'TYPE', priority: 'PRIORITY', assignee_id: 'ASSIGNED', reviewer_id: 'REVIEWER', due_at: 'DUE', recurrence: 'RECURRENCE', tags: '' }[f];
      if (f === 'assignee_id') t.assigned_at = p.assignee_id ? at : null;
      if (action) ev(id, action, typeof p[f] === 'string' ? (p[f] as string) : null);
    }
    if (done && t.recurrence) {
      const n = mk(0, { ...structuredClone(t), id: `t-${uid()}`, key: `WRK-${seq}`, status: 'TODO', started_at: null, completed_at: null, created_at: at, updated_at: at, assigned_at: at, row_version: 1, recurred_from: t.id, recurrence: { ...t.recurrence, next_at: nextAt(t.recurrence.freq, t.recurrence.interval, Date.now()) } });
      for (const l of links.filter(x => x.task === t.id && !x.archived)) links.push({ ...l, id: `l-${uid()}`, task: n.id });
      ev(id, 'RECURRED', n.key);
    }
    t.updated_at = at;
    t.row_version++;
    return wait(detail(id));
  },
  archiveTask: (id: string, reason = '') => {
    const t = tasks.find(x => x.id === id) ?? fail('NOT_FOUND', 404);
    if (t.archived_at) fail('CONFLICT', 409);
    const at = new Date().toISOString();
    const ids = tasks.filter(x => (x.id === id || x.parent_id === id) && !x.archived_at).map(x => { x.archived_at = at; x.row_version++; ev(x.id, 'ARCHIVED', reason || null); return x.id; });
    return wait({ archived: ids });
  },
  linkTask: (id: string, l: LinkInput) => { addLink(id, l); return wait(detail(id)); },
  unlink: (linkId: string) => {
    const l = links.find(x => x.id === linkId && !x.archived) ?? fail('NOT_FOUND', 404);
    l.archived = true;
    ev(l.task, 'UNLINKED', `${l.kind}:${l.entity}`);
    return wait(detail(l.task));
  },

  channels: (archived = false) => wait({
    items: channels.filter(c => (archived ? !!c.archived_at : !c.archived_at)).map(c => {
      const msgs = notes.filter(n => n.target_kind === 'channel' && n.target_id === c.id && !n.archived_at);
      const read = channelReads[c.id] ?? '';
      const unread = msgs.filter(n => n.author_id !== ME && n.created_at > read);
      return { ...c, last_read_at: read || null, last_message_at: msgs.map(n => n.created_at).sort().at(-1) ?? null, unread: unread.length, mentions: unread.filter(n => n.mentions.includes(ME)).length };
    }),
  }),
  createChannel: (body: { name: string; topic: string }) => {
    const name = body.name.replace(/^#/, '').trim();
    if (!name) fail('NAME_REQUIRED', 400);
    if (channels.some(c => !c.archived_at && c.name.toLowerCase() === name.toLowerCase())) fail('CHANNEL_EXISTS', 400);
    const c: Channel = { id: `ch-${uid()}`, name, topic: body.topic.trim(), created_by: ME, created_at: new Date().toISOString(), archived_at: null };
    channels.push(c);
    return wait(c);
  },
  updateChannel: (id: string, body: { name: string; topic: string }) => {
    const c = channels.find(x => x.id === id) ?? fail('NOT_FOUND', 404);
    Object.assign(c, { name: body.name.replace(/^#/, '').trim(), topic: body.topic.trim() });
    return wait(c);
  },
  archiveChannel: (id: string) => { const c = channels.find(x => x.id === id) ?? fail('NOT_FOUND', 404); c.archived_at = new Date().toISOString(); return wait(c); },
  readChannel: (id: string) => { channelReads[id] = new Date().toISOString(); return wait({ read: id }); },
  messages: (id: string, before?: string, latest = 60) => {
    const all = notes.filter(n => n.target_kind === 'channel' && n.target_id === id && !n.archived_at && (!before || n.created_at < before))
      .sort((a, b) => a.created_at.localeCompare(b.created_at)).map(n => ({ ...n, revisions: revisions[n.id]?.length ?? 0 }));
    return wait({ items: all.slice(-latest), has_more: all.length > latest });
  },
  notes: (kind: EntityKind, target: string, archived = false) =>
    wait({ items: notes.filter(n => n.target_kind === kind && n.target_id === target && (archived || !n.archived_at)).map(n => ({ ...n, revisions: revisions[n.id]?.length ?? 0 })) }),
  addNote: (body: NoteInput) => {
    if (!body.body.trim() && body.attachments.length === 0) fail('NOTE_EMPTY', 400);
    if (resolve(body.target_kind, body.target_id).missing) fail('ENTITY_NOT_FOUND', 400);
    const atts = body.attachments.map(a => {
      const x = attachments.get(a);
      if (!x || x.used) return fail('ATTACHMENT_NOT_READY', 400);
      x.used = true;
      return { id: x.id, name: x.name, size: x.size, content_type: x.content_type };
    });
    const n: WorkNote = { id: `n-${uid()}`, target_kind: body.target_kind, target_id: body.target_id, author_id: ME, body: body.body.trim(), mentions: body.mentions, attachments: atts, resolved: false, resolved_at: null, resolved_by: null, edited_at: null, revisions: 0, created_at: new Date().toISOString(), archived_at: null };
    notes.push(n);
    if (body.target_kind === 'task') ev(body.target_id, 'NOTE');
    return wait(n);
  },
  editNote: (id: string, body: { body: string; mentions: string[] }) => {
    const n = notes.find(x => x.id === id) ?? fail('NOT_FOUND', 404);
    if (n.author_id !== ME) fail('FORBIDDEN', 403);
    if (n.body !== body.body.trim() || JSON.stringify(n.mentions) !== JSON.stringify(body.mentions)) {
      (revisions[id] ??= []).unshift({ body: n.body, mentions: n.mentions, edited_by: ME, edited_at: new Date().toISOString() });
      n.body = body.body.trim(); n.mentions = body.mentions; n.edited_at = new Date().toISOString();
    }
    return wait({ ...n, revisions: revisions[id]?.length ?? 0 });
  },
  noteRevisions: (id: string) => wait({ items: revisions[id] ?? [] }),
  resolveNote: (id: string, resolved: boolean) => {
    const n = notes.find(x => x.id === id) ?? fail('NOT_FOUND', 404);
    n.resolved = resolved; n.resolved_by = resolved ? ME : null; n.resolved_at = resolved ? new Date().toISOString() : null;
    return wait({ ...n, revisions: revisions[id]?.length ?? 0 });
  },
  archiveNote: (id: string) => {
    const n = notes.find(x => x.id === id) ?? fail('NOT_FOUND', 404);
    if (n.author_id !== ME && people.find(p => p.user_id === ME)?.role !== 'ADMIN') fail('FORBIDDEN', 403);
    n.archived_at = new Date().toISOString();
    return wait({ id });
  },
  mentions: (resolved = false) => wait({
    items: notes.filter(n => n.mentions.includes(ME) && !n.archived_at && (resolved || !n.resolved))
      .sort((a, b) => b.created_at.localeCompare(a.created_at))
      .map(n => ({ ...n, revisions: revisions[n.id]?.length ?? 0, target: resolve(n.target_kind, n.target_id) })),
  }),
  upload: async (file: File) => {
    const id = `att-${uid()}`;
    attachments.set(id, { id, name: file.name, size: file.size, content_type: file.type || 'application/octet-stream', used: false });
    return wait(id);
  },
  attachmentUrl: () => '',
  ...makeOps({ me: ME, tasks, notes, createTask: i => mockWork.createTask(i) }),
  ...makeCrm({ me: ME, others: [KIM, TAE], resolve, exceptions: mockExceptions }),
  ...makeBoards({ me: ME, tasks, notes, resolve, createTask: i => mockWork.createTask(i), updateTask: (id, p) => mockWork.updateTask(id, p) }),
  ...makeDocs({ me: ME, tasks, notes, resolve }),
};
