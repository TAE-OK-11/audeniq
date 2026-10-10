// 체험(목) 빌드용 Phase 2 API — 업무함·예외 처리·SLA. 업무와 메모는 mock.ts의 예시 데이터를 그대로 읽어
// 서버(crates/core/src/work/{inbox,exceptions,sla}.rs)와 같은 규칙으로 흉내 낸다.
import { ApiError, messageForCode } from '../../api/errors';
import type {
  ExceptionActInput, ExceptionItem, ExceptionQuery, ExceptionReportInput, InboxCategory, InboxItem, InboxQuery, Priority,
  SlaOverview, SlaPolicy, SlaPolicyInput, SlaState, TaskDetail, TaskInput, WorkCalendar, WorkNote, WorkTask,
} from './types';

interface Ctx { me: string; tasks: WorkTask[]; notes: WorkNote[]; createTask: (i: TaskInput) => Promise<TaskDetail> }

const H = 3_600_000;
const wait = <T>(v: T) => new Promise<T>(r => setTimeout(() => r(structuredClone(v)), 140));
const fail = (code: string, status = 422): never => { throw new ApiError(messageForCode(code, status), status, code); };
const iso = (hoursAgo: number) => new Date(Date.now() - hoursAgo * H).toISOString();

let shared: ExceptionItem[] = [];
/** 체험 예외 목록 (DSP 운영 화면이 같이 읽는다) */
export const mockExceptions = () => shared;

export function makeOps(ctx: Ctx) {
  const reads = new Set<string>();
  let polSeq = 1;
  const pol = (type: SlaPolicy['type'], target: number, cal: string | null, idle: number, esc: SlaPolicy['escalate_to'], scope?: string): SlaPolicy => ({
    id: `p-${polSeq++}`, type, scope_kind: scope ? 'dsp' : null, scope_id: scope ?? null, target_minutes: target, calendar_id: cal,
    warn_ratio: 0.25, idle_alert_minutes: idle, escalate_to: esc, auto_raise_priority: true, enabled: true, updated_at: iso(200),
  });
  const policies: SlaPolicy[] = [
    pol('URGENT_DELIVERY', 120, null, 30, 'OPERATOR'), pol('RIGHTS', 540, 'KR_BUSINESS', 120, 'REVIEWER'),
    pol('RELEASE', 1080, 'KR_BUSINESS', 240, 'REVIEWER'), pol('CONTRACT', 1620, 'KR_BUSINESS', 540, 'ADMIN'),
    pol('DSP_INQUIRY', 540, 'KR_BUSINESS', 120, 'OPERATOR'), pol('SETTLEMENT', 1620, 'KR_BUSINESS', 540, 'ADMIN'),
    pol('SUPPORT', 540, 'KR_BUSINESS', 60, 'SUPPORT'), pol('LEGAL', 2700, 'KR_BUSINESS', 540, 'ADMIN'),
    pol('GENERAL', 2700, 'KR_BUSINESS', 1080, 'ADMIN'), pol('RELEASE_REVIEW', 1080, 'KR_BUSINESS', 240, 'REVIEWER'),
    pol('SECOND_APPROVAL', 540, 'KR_BUSINESS', 120, 'REVIEWER'), pol('DOCUMENT_REVIEW', 540, 'KR_BUSINESS', 120, 'REVIEWER'),
    pol('SUPPORT_INQUIRY', 540, 'KR_BUSINESS', 60, 'SUPPORT'), pol('EXCEPTION', 240, 'KR_BUSINESS', 60, 'OPERATOR'),
    pol('EXCEPTION', 60, null, 30, 'OPERATOR', 'D-5'),
  ];
  const calendars: WorkCalendar[] = [{
    id: 'KR_BUSINESS', name: '한국 영업일 (평일 09–18시)', utc_offset_minutes: 540, work_start: '09:00', work_end: '18:00',
    workdays: [1, 2, 3, 4, 5], holidays: ['2026-10-09', '2026-12-25'],
  }];
  const holidayNames: Record<string, string> = { '2026-10-09': '한글날', '2026-12-25': '성탄절' };

  const exc = (v: Partial<ExceptionItem> & Pick<ExceptionItem, 'source' | 'source_id' | 'kind' | 'severity' | 'title'>, hoursAgo: number, dueIn: number, state: SlaState): ExceptionItem => ({
    id: `${v.source}:${v.source_id}`, detail: '', org_id: null, org_name: null, dsp: null, dsp_name: null, entity: null, package_id: null,
    detected_at: iso(hoursAgo), triage: null, closed: false, retryable: false, can_act: true,
    sla: { due_at: iso(-dueIn), state }, ...v,
  });
  const exceptions: ExceptionItem[] = [
    exc({ source: 'delivery_job', source_id: 'j-71a2', kind: 'DISTRIBUTION_FAILURE', severity: 'CRITICAL', title: 'Paper Moon', detail: 'DEAD_LETTER · SFTP 연결 거부 (5회 재시도 후 중단)', org_name: 'Lumi Records', dsp: 'D-1', dsp_name: '멜론', entity: { kind: 'release', id: 'r4d5e6f7' }, package_id: 'pk-1', retryable: true }, 5, -1, 'BREACHED'),
    exc({ source: 'reconciliation', source_id: 'rc-19f0', kind: 'DSP_REJECTION', severity: 'HIGH', title: 'Blue Hour', detail: 'PARTNER_REJECTED · ARTIST_NAME_FORMAT', org_name: 'Nova Sound', dsp: 'D-5', dsp_name: 'Spotify', entity: { kind: 'release', id: 'r2b3c4d5' }, package_id: 'pk-2', retryable: true }, 0.7, 0.3, 'WARNING'),
    exc({ source: 'staging', source_id: 'pk-3:D-6', kind: 'METADATA_CONFLICT', severity: 'HIGH', title: '여름 끝에서', detail: 'DSP_GENRE_UNMAPPED, DSP_TITLE_VERSION_MISMATCH', org_name: '한결 뮤직', dsp: 'D-6', dsp_name: 'Apple Music', entity: { kind: 'release', id: 'r3c4d5e6' }, package_id: 'pk-3', retryable: true }, 2, 2, 'OK'),
    exc({ source: 'check', source_id: 'cr-5521', kind: 'QC_FAILURE', severity: 'MEDIUM', title: '고요한 밤의 노래', detail: 'AUDIO_LOUDNESS_RANGE · -5.1 LUFS (기준 -9 이하)', org_name: 'Lumi Records', entity: { kind: 'release', id: 'r5e6f7a8' } }, 8, 1, 'WARNING'),
    exc({ source: 'document', source_id: 'doc-3', kind: 'MISSING_RIGHTS_DOCUMENT', severity: 'MEDIUM', title: '공동 작곡 권리 확인서', detail: 'Blue Hour · 보완 요청 후 응답 없음', org_name: 'Nova Sound', entity: { kind: 'document', id: 'doc-3' } }, 96, -40, 'BREACHED'),
    exc({ source: 'report', source_id: 'rp-1', kind: 'SETTLEMENT_MISMATCH', severity: 'MEDIUM', title: 'apple_2026-09.tsv', detail: '매칭되지 않은 줄 14개', dsp: 'D-6', dsp_name: 'Apple Music', entity: { kind: 'settlement_report', id: 'rp-1' } }, 30, 3, 'OK'),
    exc({ source: 'job', source_id: 'job-8c1d', kind: 'AUTOMATION_FAILURE', severity: 'HIGH', title: 'stage2.fingerprint', detail: 'qc · upstream timeout', entity: { kind: 'release', id: 'r1a2b3c4' } }, 1, 3, 'OK'),
  ];
  shared = exceptions;
  exceptions[0].triage = { assignee_id: ctx.me, status: 'ACKNOWLEDGED', resolution: null, resolved_at: null, resolved_by: null, retry_count: 1, last_retry_at: iso(3), task_id: null, updated_at: iso(3) };
  const resolved: ExceptionItem[] = [];

  const deadlineOf = (t: WorkTask): [string | null, SlaState | null] => {
    const d = [t.due_at, t.sla_due_at].filter(Boolean).sort()[0] ?? null;
    if (!d || t.status === 'DONE' || t.status === 'CANCELLED') return [d, null];
    const left = Date.parse(d) - Date.now();
    const total = Math.max(1, Date.parse(d) - Date.parse(t.created_at));
    return [d, left <= 0 ? 'BREACHED' : left / total <= 0.25 ? 'WARNING' : 'OK'];
  };

  const inboxItems = (): InboxItem[] => {
    const out: InboxItem[] = [];
    for (const t of ctx.tasks) {
      if (t.archived_at || t.status === 'DONE' || t.status === 'CANCELLED') continue;
      const mine = t.assignee_id === ctx.me;
      const review = t.reviewer_id === ctx.me && t.status === 'IN_REVIEW';
      if (!mine && !review) continue;
      const [due, state] = deadlineOf(t);
      const category: InboxCategory = review ? 'REVIEW_REQUEST' : state === 'BREACHED' ? 'OVERDUE' : state === 'WARNING' ? 'SLA_WARNING' : 'ASSIGNED';
      out.push({
        id: `task:${t.id}@${t.row_version}`, category, title: t.title, subtitle: `${t.key} · ${t.status}`, type: t.type, priority: t.priority,
        due_at: due, sla_state: state, at: t.updated_at, link: `/admin/tasks/${t.id}`, entity: { kind: 'task', id: t.id },
        flags: state === 'BREACHED' ? ['ESCALATED'] : [], read: false,
      });
    }
    for (const n of ctx.notes) {
      if (n.archived_at || n.resolved || !n.mentions.includes(ctx.me) || n.author_id === ctx.me) continue;
      out.push({
        id: `note:${n.id}@0`, category: 'MENTION', title: n.body.slice(0, 160), subtitle: '', type: 'NOTE', priority: 'NORMAL',
        due_at: null, sla_state: null, at: n.created_at, link: n.target_kind === 'task' ? `/admin/tasks/${n.target_id}` : `/admin/reviews/${n.target_id}`,
        entity: { kind: n.target_kind, id: n.target_id }, flags: [], read: false,
      });
    }
    out.push(
      { id: 'second_approval:a1@1', category: 'APPROVAL_REQUEST', title: '새벽의 온도', subtitle: '한결 뮤직', type: 'SECOND_APPROVAL', priority: 'HIGH', due_at: iso(-1.5), sla_state: 'WARNING', at: iso(7), link: '/admin/approvals', entity: null, flags: [], read: false },
      { id: 'support_inquiry:iq-2@1', category: 'SUPPORT_ESCALATION', title: '지급 계좌 변경 문의', subtitle: 'Lumi Records', type: 'SUPPORT_INQUIRY', priority: 'URGENT', due_at: iso(3), sla_state: 'BREACHED', at: iso(13), link: '/admin/inquiries/iq-2', entity: { kind: 'inquiry', id: 'iq-2' }, flags: [], read: false },
    );
    for (const e of exceptions) {
      if (e.closed || !(e.triage?.assignee_id === ctx.me || (!e.triage?.assignee_id && (e.severity === 'CRITICAL' || e.severity === 'HIGH' || e.sla?.state !== 'OK')))) continue;
      const category: InboxCategory = e.kind === 'AUTOMATION_FAILURE' ? 'FAILED_AUTOMATION' : e.kind === 'MISSING_RIGHTS_DOCUMENT' ? 'RIGHTS_ISSUE'
        : e.kind === 'QC_FAILURE' || e.kind === 'SETTLEMENT_MISMATCH' ? 'EXCEPTION' : 'DISTRIBUTION_FAILURE';
      out.push({
        id: `exception:${e.id}@0`, category, title: e.title ?? '', subtitle: [e.org_name, e.dsp, e.detail].filter(Boolean).join(' · '), type: e.kind ?? '',
        priority: (({ CRITICAL: 'URGENT', HIGH: 'HIGH', LOW: 'LOW' } as Record<string, Priority>)[e.severity ?? ''] ?? 'NORMAL'),
        due_at: e.sla?.due_at ?? null, sla_state: e.sla?.state ?? null, at: e.detected_at ?? iso(0),
        link: `/admin/exceptions?focus=${encodeURIComponent(e.id)}`, entity: e.entity ?? null, flags: e.triage?.assignee_id === ctx.me ? ['ASSIGNED'] : [], read: false,
      });
    }
    const r = (p: string) => ['URGENT', 'HIGH', 'NORMAL', 'LOW'].indexOf(p);
    return out.map(i => ({ ...i, read: reads.has(i.id) })).sort((a, b) => r(a.priority) - r(b.priority) || String(a.due_at ?? '9').localeCompare(String(b.due_at ?? '9')));
  };

  const sla = (): SlaOverview => {
    const board = (['RELEASE_REVIEW', 'SECOND_APPROVAL', 'DOCUMENT_REVIEW', 'SUPPORT_INQUIRY'] as const).map((queue, i) => ({ queue, open: [4, 1, 2, 3][i], warning: [1, 1, 0, 0][i], breached: [0, 0, 1, 1][i] }));
    const tasks = ctx.tasks.filter(t => !t.archived_at && t.status !== 'DONE' && t.status !== 'CANCELLED').map(t => {
      const [deadline, state] = deadlineOf(t);
      return { id: t.id, key: t.key, title: t.title, type: t.type, priority: t.priority, assignee_id: t.assignee_id, deadline, state, escalation_level: state === 'BREACHED' ? 1 : 0 };
    }).filter(t => t.deadline).sort((a, b) => String(a.deadline).localeCompare(String(b.deadline)));
    return {
      policies: policies.filter(p => p.id), calendars, holiday_names: holidayNames, board,
      queue: [
        { queue: 'DOCUMENT_REVIEW', id: 'doc-2', title: '샘플 사용 허락서', org_name: 'Nova Sound', start: iso(30), due_at: iso(6), state: 'BREACHED', escalate_to: 'REVIEWER', link: '/admin/documents', owner_id: null },
        { queue: 'SUPPORT_INQUIRY', id: 'iq-2', title: '지급 계좌 변경 문의', org_name: 'Lumi Records', start: iso(13), due_at: iso(3), state: 'BREACHED', escalate_to: 'SUPPORT', link: '/admin/inquiries/iq-2', owner_id: null },
        { queue: 'SECOND_APPROVAL', id: 'a1', title: '새벽의 온도', org_name: '한결 뮤직', start: iso(7), due_at: iso(-1.5), state: 'WARNING', escalate_to: 'REVIEWER', link: '/admin/approvals', owner_id: null },
        { queue: 'RELEASE_REVIEW', id: 'r2b3c4d5', title: 'Blue Hour', org_name: 'Nova Sound', start: iso(20), due_at: iso(-4), state: 'WARNING', escalate_to: 'REVIEWER', link: '/admin/reviews/r2b3c4d5', owner_id: null },
      ],
      tasks,
    };
  };

  return {
    summary: () => {
      const items = inboxItems();
      const open = exceptions.filter(e => !e.closed);
      return wait({
        inbox: { total: items.length, unread: items.filter(i => !i.read).length },
        overdue: items.filter(i => i.category === 'OVERDUE').length,
        sla_warning: items.filter(i => i.category === 'SLA_WARNING').length,
        exceptions: { open: open.length, mine: open.filter(e => e.triage?.assignee_id === ctx.me).length, critical: open.filter(e => e.severity === 'CRITICAL').length },
      });
    },
    inbox: (q: InboxQuery = {}) => {
      const all = inboxItems();
      const counts: Record<string, { total: number; unread: number }> = {};
      for (const i of all) { const c = (counts[i.category] ??= { total: 0, unread: 0 }); c.total += 1; if (!i.read) c.unread += 1; }
      const now = Date.now();
      const items = all.filter(i => (!q.category || i.category === q.category) && (!q.priority || i.priority === q.priority)
        && (!q.unread || !i.read) && (!q.entity_kind || i.entity?.kind === q.entity_kind)
        && (!q.due || (i.due_at && Date.parse(i.due_at) < (q.due === 'overdue' ? now : q.due === 'today' ? new Date().setHours(24, 0, 0, 0) : now + 7 * 24 * H))));
      return wait({ items, counts, total: all.length, unread: all.filter(i => !i.read).length });
    },
    markRead: (ids: string[]) => { ids.forEach(i => reads.add(i)); return wait({ read: ids.length }); },
    exceptions: (q: ExceptionQuery = {}) => {
      if (q.view === 'resolved') return wait({ items: resolved, counts: {} });
      const open = exceptions.filter(e => !e.closed);
      const counts: Record<string, number> = {};
      for (const e of open) if (e.kind) counts[e.kind] = (counts[e.kind] ?? 0) + 1;
      const term = q.q?.toLowerCase();
      const items = open.filter(e => (q.view !== 'mine' || e.triage?.assignee_id === ctx.me) && (q.view !== 'unassigned' || !e.triage?.assignee_id)
        && (!q.kind || e.kind === q.kind) && (!q.severity || e.severity === q.severity)
        && (!term || [e.title, e.detail, e.org_name].some(v => v?.toLowerCase().includes(term))));
      return wait({ items, counts, total: open.length, mine: open.filter(e => e.triage?.assignee_id === ctx.me).length });
    },
    actException: async (source: string, sourceId: string, body: ExceptionActInput) => {
      const e = exceptions.find(x => x.source === source && x.source_id === sourceId) ?? fail('NOT_FOUND', 404);
      const t = (e.triage ??= { assignee_id: null, status: null, resolution: null, resolved_at: null, resolved_by: null, retry_count: 0, last_retry_at: null, task_id: null, updated_at: iso(0) });
      t.updated_at = iso(0);
      switch (body.action) {
        case 'ASSIGN': t.assignee_id = body.assignee_id ?? null; break;
        case 'ACK': t.status = 'ACKNOWLEDGED'; t.assignee_id ??= ctx.me; break;
        case 'RESOLVE': case 'IGNORE':
          if (body.action === 'IGNORE' && !body.resolution?.trim()) fail('RESOLUTION_REQUIRED', 400);
          t.status = body.action === 'RESOLVE' ? 'RESOLVED' : 'IGNORED'; t.resolution = body.resolution ?? null; t.resolved_at = iso(0); t.resolved_by = ctx.me;
          e.closed = true; resolved.unshift(structuredClone(e)); break;
        case 'REOPEN': t.status = null; t.resolved_at = null; e.closed = false; break;
        case 'RETRY':
          if (!e.retryable) fail('EXCEPTION_NOT_RETRYABLE');
          if (t.last_retry_at && Date.now() - Date.parse(t.last_retry_at) < 120_000) fail('RETRY_TOO_SOON');
          t.retry_count += 1; t.last_retry_at = iso(0); t.status ??= 'ACKNOWLEDGED'; t.assignee_id ??= ctx.me; break;
        case 'TO_TASK': {
          if (t.task_id) fail('EXCEPTION_HAS_TASK');
          const type = e.kind === 'DSP_REJECTION' || e.kind === 'DISTRIBUTION_FAILURE' ? 'URGENT_DELIVERY' : e.kind === 'MISSING_RIGHTS_DOCUMENT' ? 'RIGHTS'
            : e.kind === 'SETTLEMENT_MISMATCH' ? 'SETTLEMENT' : e.kind === 'AUTOMATION_FAILURE' ? 'GENERAL' : 'RELEASE';
          const d = await ctx.createTask({
            title: `[예외] ${e.title}`, description: `${e.detail}\n\n출처: ${e.source} · ${e.source_id}`, type,
            priority: e.severity === 'CRITICAL' ? 'URGENT' : e.severity === 'HIGH' ? 'HIGH' : 'NORMAL', assignee_id: t.assignee_id ?? ctx.me,
            links: e.entity ? [{ entity_kind: e.entity.kind, entity_id: e.entity.id }] : [], tags: ['예외'],
          });
          t.task_id = d.task.id; t.status ??= 'ACKNOWLEDGED'; t.assignee_id ??= ctx.me; break;
        }
      }
      return wait({ item: e });
    },
    reportException: (body: ExceptionReportInput) => {
      if (!body.title.trim()) fail('TITLE_REQUIRED', 400);
      const id = Math.random().toString(16).slice(2, 10);
      exceptions.unshift(exc({ source: 'manual', source_id: id, kind: body.kind, severity: body.severity, title: body.title, detail: body.detail,
        entity: body.entity_kind && body.entity_id ? { kind: body.entity_kind, id: body.entity_id } : null }, 0, 4, 'OK'));
      return wait({ id: `manual:${id}` });
    },
    sla: () => wait(sla()),
    savePolicy: (body: SlaPolicyInput) => {
      const dup = policies.find(p => p.type === body.type && (p.scope_id ?? '') === (body.scope_id ?? '') && p.id !== body.id);
      if (dup) fail('SLA_POLICY_EXISTS', 400);
      const next = { ...body, id: body.id ?? `p-${polSeq++}`, updated_at: iso(0) } as SlaPolicy;
      const i = policies.findIndex(p => p.id === next.id);
      if (i >= 0) policies[i] = next; else policies.push(next);
      return wait(sla());
    },
    archivePolicy: (id: string) => { const i = policies.findIndex(p => p.id === id); if (i < 0) fail('NOT_FOUND', 404); policies.splice(i, 1); return wait(sla()); },
    saveCalendar: (id: string, body: { name: string; work_start: string; work_end: string; workdays: number[] }) => {
      const c = calendars.find(x => x.id === id) ?? fail('NOT_FOUND', 404);
      if (body.work_end <= body.work_start) fail('WORK_HOURS_INVALID', 400);
      Object.assign(c, body);
      return wait(sla());
    },
    setHoliday: (id: string, day: string, name?: string) => {
      const c = calendars.find(x => x.id === id) ?? fail('NOT_FOUND', 404);
      c.holidays = c.holidays.filter(d => d !== day);
      delete holidayNames[day];
      if (name) { c.holidays = [...c.holidays, day].sort(); holidayNames[day] = name; }
      return wait(sla());
    },
  };
}
