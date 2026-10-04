// 체험(목) 빌드용 Phase 3 API — CRM Lite · DSP 운영. 서버(crates/core/src/work/{crm,dsp_ops}.rs)처럼
// 이름·계약 상태는 원천(예시 원천)에서 읽고, 관계에는 담당자·상태·연락처·연락 이력만 둔다.
import { ApiError, messageForCode } from '../../api/errors';
import { DSP_NAME } from '../dspReqs';
import type {
  ActivityInput, ContactInput, DspOps, DspOpsItem, EntityRef, ExceptionItem, LinkInput, RelActivity, RelContact, RelContract,
  RelInput, RelPatch, RelQuery, Relationship, RelationshipDetail,
} from './types';

interface Ctx { me: string; others: string[]; resolve: (kind: EntityRef['kind'], id: string) => EntityRef; exceptions: () => ExceptionItem[] }

const H = 3_600_000;
const wait = <T>(v: T) => new Promise<T>(r => setTimeout(() => r(structuredClone(v)), 140));
const fail = (code: string, status = 422): never => { throw new ApiError(messageForCode(code, status), status, code); };
const iso = (hoursAgo: number) => new Date(Date.now() - hoursAgo * H).toISOString();
const uid = () => Math.random().toString(16).slice(2, 10);

// 원천 계약 상태 (예시): 작업 공간의 배급 계약서, DSP 계약 경로
const AGREEMENTS: Record<string, RelContract> = {
  'org-r4': { status: 'SIGNED', source: 'agreements', signed: 3, pending: 1, updated_at: iso(50) },
  'org-y': { status: 'NEGOTIATING', source: 'agreements', signed: 0, pending: 1, updated_at: iso(20) },
};
const DSP_CONTRACT: Record<string, RelContract> = {
  'D-1': { status: 'SIGNED', source: 'dsp_contract', route: 'DIRECT', onboarding_stage: 'LIVE' },
  'D-5': { status: 'SIGNED', source: 'dsp_contract', route: 'MERLIN', onboarding_stage: 'LIVE' },
  'D-6': { status: 'NEGOTIATING', source: 'dsp_contract', route: 'DIRECT', onboarding_stage: 'TESTING' },
};

export function makeCrm(ctx: Ctx) {
  const [KIM, TAE] = ctx.others;
  interface Row { r: Relationship; contacts: (RelContact & { archived?: boolean })[]; activities: RelActivity[]; links: (EntityRef & { link_id: string; archived?: boolean })[] }
  const rows: Row[] = [];
  const contract = (r: Relationship): RelContract => {
    if (!r.entity) return { status: r.contract.status, source: 'manual' };
    if (r.entity.kind === 'dsp') return DSP_CONTRACT[r.entity.id] ?? { status: 'NONE', source: 'dsp_contract', route: null, onboarding_stage: null };
    const org = r.entity.kind === 'org' ? r.entity.id : 'org-r4';
    return AGREEMENTS[org] ?? { status: 'NONE', source: 'agreements', signed: 0, pending: 0, updated_at: null };
  };
  const view = (row: Row): Relationship => {
    const r = row.r;
    const name = r.entity ? ctx.resolve(r.entity.kind, r.entity.id).label : r.own_name;
    const contacts = row.contacts.filter(c => !c.archived);
    return {
      ...r, name, contract: contract(r),
      contact_due: !!r.next_contact_at && Date.parse(r.next_contact_at) <= Date.now(),
      last_contact_at: row.activities.map(a => a.at).sort().at(-1) ?? null,
      primary_contact: contacts.find(c => c.is_primary) ?? contacts[0] ?? null,
    };
  };
  const mk = (kind: Relationship['kind'], entity: Relationship['entity'], own: string | null, v: Partial<Relationship>,
    contacts: Omit<RelContact, 'id'>[] = [], acts: [RelActivity['kind'], string, number][] = []) => {
    const r: Relationship = {
      id: `c-${uid()}`, kind, entity, name: null, own_name: own, missing: false, owner_id: ctx.me, status: 'ACTIVE', next_contact_at: null,
      contact_due: false, last_contact_at: null, tags: [], contract: { status: 'NONE', source: entity ? 'agreements' : 'manual' }, open_tasks: 0,
      note_count: 0, primary_contact: null, org_id: null, created_by: ctx.me, created_at: iso(500), updated_at: iso(10), row_version: 1, archived_at: null, ...v,
    };
    rows.push({
      r, contacts: contacts.map((c, i) => ({ ...c, id: `ct-${uid()}`, is_primary: i === 0 })),
      activities: acts.map(([k, s, h]) => ({ id: `a-${uid()}`, kind: k, summary: s, at: iso(h), by_user: ctx.me, contact: contacts[0]?.name ?? null })),
      links: [],
    });
    return r;
  };
  const lumi = mk('ORG', { kind: 'org', id: 'org-r4' }, null, { id: 'c-lumi', tags: ['핵심'], next_contact_at: iso(2), open_tasks: 1, note_count: 1 },
    [{ name: '이서연', role: 'A&R 팀장', email: 'seoyeon@lumi.example', phone: '010-1234-5678' }, { name: '정민', role: '정산 담당', email: 'jm@lumi.example', phone: null }],
    [['MEETING', '수수료 7% 조정 요청. 다음 주 비교표 회신 약속.', 30], ['CALL', '9월 정산 일정 확인.', 200]]);
  rows.find(x => x.r.id === lumi.id)!.links.push({ ...ctx.resolve('release', 'r4d5e6f7'), link_id: 'lk-1' });
  mk('DSP', { kind: 'dsp', id: 'D-5' }, null, { owner_id: TAE, next_contact_at: iso(-72), open_tasks: 1 },
    [{ name: 'Partner Ops', role: 'Content Ops', email: 'ops@spotify.example', phone: null }], [['EMAIL', '피처링 표기 반려 건 기준 문의 — 답변 대기.', 20]]);
  mk('DSP', { kind: 'dsp', id: 'D-1' }, null, { owner_id: TAE, status: 'AT_RISK', next_contact_at: iso(26) },
    [{ name: '박지훈', role: '콘텐츠 제휴', email: 'jh@melon.example', phone: '02-555-0101' }], [['CALL', 'SFTP 접속 거부 장애 공유, 방화벽 확인 요청.', 6]]);
  mk('ARTIST', { kind: 'artist', id: 'ar-2' }, null, { owner_id: KIM, status: 'ACTIVE', tags: ['신규'] }, [], [['NOTE', '데뷔 싱글 일정 협의.', 100]]);
  mk('PARTNER', null, '모노 퍼블리싱', { status: 'PROSPECT', contract: { status: 'NEGOTIATING', source: 'manual' }, next_contact_at: iso(-120), tags: ['퍼블리싱'] },
    [{ name: '한지수', role: '대표', email: 'js@mono.example', phone: null }]);

  const find = (id: string) => rows.find(x => x.r.id === id) ?? fail('NOT_FOUND', 404);
  const detail = (id: string): RelationshipDetail => {
    const row = find(id);
    const isOrg = row.r.entity && row.r.entity.kind !== 'dsp';
    return {
      relationship: view(row),
      contacts: row.contacts.filter(c => !c.archived),
      activities: [...row.activities].sort((a, b) => b.at.localeCompare(a.at)),
      links: row.links.filter(l => !l.archived),
      releases: isOrg ? [
        { id: 'r4d5e6f7', title: 'Paper Moon', status: 'READY_FOR_DELIVERY', created_at: iso(300) },
        { id: 'r5e6f7a8', title: '고요한 밤의 노래', status: 'STAGE2_REVIEW', created_at: iso(90) },
      ] : [],
      agreements: isOrg ? [{ id: 'doc-1', title: 'Paper Moon · 배급 신청·계약서', status: 'SIGNED', release_title: 'Paper Moon', updated_at: iso(50) }] : [],
    };
  };
  const live = (id: string) => { const row = find(id); if (row.r.archived_at) fail('RELATIONSHIP_ARCHIVED'); return row; };
  const bump = (row: Row) => { row.r.updated_at = new Date().toISOString(); row.r.row_version += 1; };

  const dspItem = (code: string, i: number): DspOpsItem => {
    const total = [42, 0, 0, 0, 18, 37, 12][i % 7];
    const failed = [5, 0, 0, 0, 1, 0, 2][i % 7];
    const excs = ctx.exceptions().filter(e => !e.closed && e.dsp === code);
    const rel = rows.find(x => !x.r.archived_at && x.r.entity?.kind === 'dsp' && x.r.entity.id === code);
    return {
      code, name: DSP_NAME[code],
      route: { transport: i % 2 ? 'SFTP' : 'HTTP_API', delivery_enabled: total > 0, onboarding_stage: DSP_CONTRACT[code]?.onboarding_stage ?? null, onboarding_gaps: total ? [] : ['contract_signed'] },
      contract_route: { route: DSP_CONTRACT[code]?.route ?? 'DIRECT', contract_live: DSP_CONTRACT[code]?.status === 'SIGNED' },
      metrics: { total, delivered: total - failed - (total ? 2 : 0), failed, in_flight: total ? 2 : 0, failure_rate: total ? failed / total : 0, rejected: code === 'D-5' ? 3 : 0, avg_minutes: total ? 18 + i * 7 : null, last_delivered_at: total ? iso(i + 1) : null, last_failure_at: failed ? iso(5) : null },
      staging: { pending: i % 3, blocked: code === 'D-6' ? 1 : 0 },
      exceptions: { open: excs.length, critical: excs.filter(e => e.severity === 'CRITICAL').length, high: excs.filter(e => e.severity === 'HIGH').length },
      relationship: rel ? view(rel) : null,
      sla: code === 'D-5' ? [{ type: 'EXCEPTION', target_minutes: 60, calendar_id: null }] : [],
    };
  };

  return {
    relationships: (q: RelQuery = {}) => {
      const term = q.q?.trim().toLowerCase();
      const items = rows.map(view).filter(r => (q.archived ? !!r.archived_at : !r.archived_at) && (!q.kind || r.kind === q.kind) && (!q.status || r.status === q.status)
        && (!q.owner || r.owner_id === q.owner) && (!q.contact || (r.next_contact_at && Date.parse(r.next_contact_at) < (q.contact === 'due' ? Date.now() + 1 : Date.now() + 7 * 24 * H)))
        && (!term || (r.name ?? '').toLowerCase().includes(term) || r.tags.includes(q.q!.trim())))
        .sort((a, b) => String(a.next_contact_at ?? '9').localeCompare(String(b.next_contact_at ?? '9')));
      const counts: Record<string, number> = {};
      for (const r of rows) if (!r.r.archived_at) counts[r.r.kind] = (counts[r.r.kind] ?? 0) + 1;
      return wait({ items, counts });
    },
    relationship: (id: string) => wait(detail(id)),
    createRelationship: (body: RelInput) => {
      if (body.entity_id && rows.some(x => !x.r.archived_at && x.r.entity?.id === body.entity_id)) fail('RELATIONSHIP_EXISTS', 400);
      if (!body.entity_id && !body.name?.trim()) fail('NAME_REQUIRED', 400);
      const r = mk(body.kind, body.entity_kind && body.entity_id ? { kind: body.entity_kind as 'org', id: body.entity_id } : null, body.entity_id ? null : body.name!.trim(), {
        owner_id: body.owner_id ?? null, status: body.status ?? 'ACTIVE', next_contact_at: body.next_contact_at ?? null, tags: body.tags ?? [],
        contract: { status: body.manual_contract_status ?? 'NONE', source: 'manual' }, created_at: iso(0), updated_at: iso(0),
      });
      return wait(detail(r.id));
    },
    updateRelationship: (id: string, p: RelPatch) => {
      const row = live(id);
      if (row.r.row_version !== p.row_version) fail('CONFLICT', 409);
      if (p.owner_id !== undefined) row.r.owner_id = p.owner_id;
      if (p.status) row.r.status = p.status;
      if (p.next_contact_at !== undefined) row.r.next_contact_at = p.next_contact_at;
      if (p.tags) row.r.tags = p.tags;
      if (p.name !== undefined) { if (row.r.entity) fail('NAME_FROM_SOURCE', 400); row.r.own_name = p.name; }
      if (p.manual_contract_status) { if (row.r.entity) fail('CONTRACT_FROM_SOURCE', 400); row.r.contract = { status: p.manual_contract_status, source: 'manual' }; }
      bump(row);
      return wait(detail(id));
    },
    archiveRelationship: (id: string) => { const row = live(id); row.r.archived_at = iso(0); return wait({ archived: id }); },
    addContact: (id: string, body: ContactInput) => {
      const row = live(id);
      if (!body.name.trim()) fail('NAME_REQUIRED', 400);
      if (body.is_primary) row.contacts.forEach(c => { c.is_primary = false; });
      row.contacts.push({ id: `ct-${uid()}`, name: body.name.trim(), role: body.role, email: body.email || null, phone: body.phone || null, is_primary: body.is_primary || !row.contacts.some(c => !c.archived) });
      bump(row);
      return wait(detail(id));
    },
    editContact: (contactId: string, body: ContactInput) => {
      const row = rows.find(x => x.contacts.some(c => c.id === contactId)) ?? fail('NOT_FOUND', 404);
      const c = row.contacts.find(x => x.id === contactId)!;
      if (body.archived) { c.archived = true; c.is_primary = false; } else {
        if (body.is_primary) row.contacts.forEach(x => { x.is_primary = false; });
        Object.assign(c, { name: body.name.trim(), role: body.role, email: body.email || null, phone: body.phone || null, is_primary: body.is_primary });
      }
      bump(row);
      return wait(detail(row.r.id));
    },
    logActivity: (id: string, body: ActivityInput) => {
      const row = live(id);
      if (!body.summary.trim()) fail('SUMMARY_REQUIRED', 400);
      row.activities.push({ id: `a-${uid()}`, kind: body.kind, summary: body.summary.trim(), at: body.at ?? new Date().toISOString(), by_user: ctx.me,
        contact: row.contacts.find(c => c.id === body.contact_id)?.name ?? null });
      if (body.next_contact_at !== undefined) row.r.next_contact_at = body.next_contact_at;
      bump(row);
      return wait(detail(id));
    },
    linkRelationship: (id: string, body: LinkInput) => {
      const row = live(id);
      const ref = ctx.resolve(body.entity_kind, body.entity_id);
      if (ref.missing) fail('ENTITY_NOT_FOUND', 400);
      if (!row.links.some(l => !l.archived && l.kind === ref.kind && l.id === ref.id)) row.links.push({ ...ref, link_id: `lk-${uid()}` });
      bump(row);
      return wait(detail(id));
    },
    unlinkRelationship: (id: string, linkId: string) => {
      const row = live(id);
      const l = row.links.find(x => x.link_id === linkId && !x.archived) ?? fail('NOT_FOUND', 404);
      l.archived = true;
      bump(row);
      return wait(detail(id));
    },
    dspOps: (dsp?: string): Promise<DspOps> => {
      const items = Object.keys(DSP_NAME).slice(0, 12).map((code, i) => {
        const it = dspItem(code, i);
        if (code === dsp) {
          it.recent_failures = it.metrics.failed ? [{ job_id: 'j-71a2', status: 'DEAD_LETTER', attempts: 5, last_error: 'SFTP 연결 거부', updated_at: iso(5), release_id: 'r4d5e6f7', title: 'Paper Moon', org_name: 'Lumi Records' }] : [];
          it.open_exceptions = ctx.exceptions().filter(e => !e.closed && e.dsp === code);
        }
        return it;
      });
      return wait({ items, window_days: 30, live_transmission: false });
    },
  };
}
