// 체험(목) 빌드용 문서 API — crates/core/src/work/docs.rs 규칙을 흉내 낸다: 버전 충돌(409), 승인 흐름(자기 승인 불가),
// 트리(깊이 5단계·순환 금지), 보관·복원·삭제(보관한 것만), 버전 기록, 공유 권한, 템플릿, 문서 댓글(답글·위치), 임베드.
import { ApiError, messageForCode } from '../../api/errors';
import { plainText } from './docs/model';
import type {
  DocComment, DocCreateInput, DocDetail, DocGrant, DocNode, DocPermissions, DocSaveInput, DocSearchQuery, DocStatusAction,
  DocSummary, DocTemplate, DocVersion, EmbedData, EntityRef, WorkNote, WorkTask,
} from './types';
import type { WorkApi } from './api';

export interface DocsCtx { me: string; tasks: WorkTask[]; notes: WorkNote[]; resolve: (kind: EntityRef['kind'], id: string) => EntityRef }
type DocApi = Pick<WorkApi, 'docTree' | 'searchDocs' | 'doc' | 'createDoc' | 'saveDoc' | 'moveDoc' | 'duplicateDoc' | 'archiveDoc'
  | 'restoreDoc' | 'deleteDoc' | 'docStatus' | 'docVersions' | 'docVersion' | 'saveDocVersion' | 'restoreDocVersion' | 'docPermissions'
  | 'setDocPermissions' | 'docTemplates' | 'saveDocTemplate' | 'archiveDocTemplate' | 'docComments' | 'addDocComment' | 'embeds'
  | 'attachmentInline'>;

const H = 3_600_000;
const iso = (hoursAgo: number) => new Date(Date.now() - hoursAgo * H).toISOString();
const wait = <T>(v: T) => new Promise<T>(r => setTimeout(() => r(structuredClone(v)), 120));
const fail = (code: string, status = 422): never => { throw new ApiError(messageForCode(code, status), status, code); };
const uid = () => `d-${Math.random().toString(16).slice(2, 10)}`;
const p = (text = ''): DocNode => (text ? { type: 'paragraph', content: [{ type: 'text', text }] } : { type: 'paragraph' });
const h2 = (text: string): DocNode => ({ type: 'heading', attrs: { level: 2 }, content: [{ type: 'text', text }] });
const todo = (...items: string[]): DocNode => ({ type: 'taskList', content: items.map(t => ({ type: 'taskItem', attrs: { checked: false }, content: [p(t)] })) });

interface Doc {
  id: string; parent_id: string | null; title: string; icon: string | null; content: DocNode; status: DocSummary['status'];
  staff_access: DocSummary['staff_access']; version: number; position: number; created_by: string; updated_by: string;
  created_at: string; updated_at: string; archived_at: string | null; deleted: boolean; review: DocSummary['review'];
  grants: DocGrant[]; template_id: string | null;
}
interface Version { doc: string; version: number; title: string; content: DocNode; reason: DocVersion['reason']; label: string | null; created_by: string; created_at: string }

const TEMPLATES: (DocTemplate & { content: DocNode })[] = [
  { id: 'tpl-meeting', key: 'meeting_notes', name: '회의록', description: '참석자·안건·결정·할 일', icon: 'pencil', system: true,
    content: { type: 'doc', content: [h2('안건'), p(), h2('결정 사항'), p(), h2('할 일'), todo('담당자 · 기한 · 내용')] } },
  { id: 'tpl-incident', key: 'incident_report', name: '장애 보고서', description: '영향·타임라인·원인·재발 방지', icon: 'siren', system: true,
    content: { type: 'doc', content: [{ type: 'callout', attrs: { tone: 'danger' }, content: [p('심각도와 영향 범위를 먼저 적어 주세요.')] }, h2('타임라인'), p(), h2('원인'), p(), h2('재발 방지'), todo('')] } },
  { id: 'tpl-dsp', key: 'dsp_issue_report', name: 'DSP 이슈 보고', description: 'DSP 전송 실패·반려 분석', icon: 'antenna', system: true,
    content: { type: 'doc', content: [h2('현상'), p(), h2('DSP 응답'), { type: 'codeBlock', content: [{ type: 'text', text: '응답 원문' }] }, h2('조치'), todo('메타데이터 수정', '재전송')] } },
  { id: 'tpl-rights', key: 'rights_review', name: '권리 검토', description: '권리 증빙·판단 근거 기록', icon: 'scale', system: true,
    content: { type: 'doc', content: [h2('검토 대상'), p(), h2('확인 사항'), todo('원권리자 서명', '사용 범위'), h2('판단'), p()] } },
  { id: 'tpl-sop', key: 'sop', name: 'SOP', description: '업무 절차서', icon: 'book', system: true,
    content: { type: 'doc', content: [h2('목적'), p(), h2('절차'), { type: 'orderedList', content: [{ type: 'listItem', content: [p()] }] }] } },
];

export function makeDocs(ctx: DocsCtx): DocApi {
  const { me } = ctx;
  const others = ['staff-kim-0002', 'staff-tae-0003'];
  const docs: Doc[] = [];
  const versions: Version[] = [];
  const comments: DocComment[] = [];
  const mk = (o: Partial<Doc> & { title: string }, hoursAgo: number): Doc => {
    const d: Doc = {
      id: uid(), parent_id: null, icon: null, content: { type: 'doc', content: [p()] }, status: 'DRAFT', staff_access: 'EDITOR', version: 1,
      position: (docs.length + 1) * 1024, created_by: me, updated_by: me, created_at: iso(hoursAgo), updated_at: iso(hoursAgo / 2),
      archived_at: null, deleted: false, grants: [], template_id: null,
      review: { requested_by: null, requested_at: null, approved_by: null, approved_at: null, rejected_by: null, rejected_at: null, rejection_reason: null },
      ...o,
    };
    docs.push(d);
    versions.push({ doc: d.id, version: 1, title: d.title, content: d.content, reason: 'CREATE', label: null, created_by: d.created_by, created_at: d.created_at });
    return d;
  };
  const ops = mk({ title: '배급 운영', icon: 'truck', content: { type: 'doc', content: [p('배급팀 문서 모음이에요.')] } }, 400);
  mk({ title: '멜론 SFTP 전송 장애 (9/28)', icon: 'siren', parent_id: ops.id, status: 'APPROVED', created_by: others[1], content: {
    type: 'doc', content: [
      { type: 'callout', attrs: { tone: 'danger' }, content: [p('P1 · 멜론 신규 발매 12건 전송 지연 (2시간 10분)')] },
      h2('영향 받은 발매'),
      { type: 'entityEmbed', attrs: { kind: 'release', id: 'r1a2b3c4', view: 'distribution', label: '새벽의 온도' } },
      h2('원인'), p('파트너 측 SFTP 인증서 교체 후 호스트 키 불일치.'),
      h2('재발 방지'), todo('호스트 키 변경 공지 채널 등록', '전송 실패 3회 연속 시 자동 카드'),
    ] } }, 150);
  mk({ title: 'DSP 반려 대응 SOP', icon: 'book', parent_id: ops.id, content: TEMPLATES[4].content }, 90);
  mk({ title: '권리 검토 기준', icon: 'scale', staff_access: 'VIEWER', content: { type: 'doc', content: [h2('샘플 사용'), p('허락서에 사용 범위가 명시돼야 해요.'), { type: 'entityEmbed', attrs: { kind: 'release', id: 'r4d5e6f7', view: 'qc', label: 'Paper Moon' } }] } }, 300);
  mk({ title: '임원 회의 메모', icon: 'lock', staff_access: null, created_by: others[0] }, 30);

  const name = (l: number) => (['NONE', 'VIEWER', 'EDITOR', 'MANAGER', 'OWNER'] as const)[l];
  const level = (d: Doc): number => {
    if (d.deleted) return 0;
    if (d.created_by === me) return 4;
    return 3; // 체험 계정은 ADMIN
  };
  const find = (id: string, need = 1) => {
    const d = docs.find(x => x.id === id);
    if (!d || level(d) === 0) return fail('NOT_FOUND', 404);
    if (level(d) < need) return fail('FORBIDDEN', 403);
    return d;
  };
  const depth = (d: Doc): number => (d.parent_id ? 1 + depth(docs.find(x => x.id === d.parent_id) ?? d) : 0);
  const summary = (d: Doc): DocSummary => ({
    id: d.id, parent_id: d.parent_id, depth: depth(d), title: d.title, icon: d.icon, status: d.status, staff_access: d.staff_access,
    version: d.version, position: d.position, template_id: d.template_id, created_by: d.created_by, updated_by: d.updated_by,
    created_at: d.created_at, updated_at: d.updated_at, archived_at: d.archived_at, review: d.review, level: name(level(d)),
    has_children: docs.some(c => c.parent_id === d.id && !c.deleted && !c.archived_at),
  });
  const embedsOf = (n: DocNode, out: { kind: string; id: string }[] = []) => {
    if (n.type === 'entityEmbed' && n.attrs?.kind && n.attrs.kind !== 'audit') out.push({ kind: String(n.attrs.kind), id: String(n.attrs.id) });
    for (const c of n.content ?? []) embedsOf(c, out);
    return out;
  };
  const detail = (d: Doc): DocDetail => {
    const crumbs: DocDetail['breadcrumb'] = [];
    for (let x = docs.find(y => y.id === d.parent_id); x; x = docs.find(y => y.id === x?.parent_id)) crumbs.unshift({ id: x.id, title: x.title, icon: x.icon });
    const rel = embedsOf(d.content).filter(e => e.kind !== 'task' && e.kind !== 'doc')
      .map(e => ({ ...ctx.resolve(e.kind as EntityRef['kind'], e.id), origin: 'embed' as const }));
    const cards = ctx.tasks.filter(t => !t.archived_at && (t.links.some(l => l.kind === 'doc' && l.id === d.id) || embedsOf(d.content).some(e => e.kind === 'task' && e.id === t.id)))
      .map(t => ({ id: t.id, key: t.key, title: t.title, status: t.status, priority: t.priority, assignee_id: t.assignee_id, due_at: t.due_at, column_name: t.column_name ?? null, board_id: t.board_id ?? null }));
    const backlinks = docs.filter(x => x.id !== d.id && !x.deleted && JSON.stringify(x.content).includes(d.id)).map(x => ({ id: x.id, title: x.title, icon: x.icon }));
    return { doc: { ...summary(d), content: d.content }, breadcrumb: crumbs, related: rel, cards, backlinks, open_comments: comments.filter(c => c.target_id === d.id && !c.parent_id && !c.resolved).length };
  };
  const snapshot = (d: Doc, reason: Version['reason'], label: string | null = null) =>
    versions.push({ doc: d.id, version: d.version, title: d.title, content: structuredClone(d.content), reason, label, created_by: d.updated_by, created_at: d.updated_at });
  const subtree = (id: string): Doc[] => { const out = docs.filter(d => d.id === id); for (const c of docs.filter(d => d.parent_id === id)) out.push(...subtree(c.id)); return out; };

  const api: DocApi = {
    docTree: (parent?: string | null, archived = false) => wait({
      items: docs.filter(d => !d.deleted && level(d) > 0 && (archived ? !!d.archived_at : !d.archived_at) && (archived || d.parent_id === (parent ?? null)))
        .sort((a, b) => a.position - b.position).map(summary),
    }),
    searchDocs: (q: DocSearchQuery) => {
      const term = (q.q ?? '').toLowerCase();
      return wait({
        items: docs.filter(d => !d.deleted && level(d) > 0 && !d.archived_at && (!term || d.title.toLowerCase().includes(term) || plainText(d.content).toLowerCase().includes(term)))
          .map(d => {
            const text = plainText(d.content);
            const at = text.toLowerCase().indexOf(term);
            return { ...summary(d), snippet: term && at >= 0 ? text.slice(Math.max(0, at - 40), at + 120) : null };
          }),
      });
    },
    doc: (id: string) => wait(detail(find(id))),
    createDoc: (body: DocCreateInput) => {
      const parent = body.parent_id ? find(body.parent_id, 2) : null;
      if (parent && depth(parent) >= 4) fail('DOC_TOO_DEEP');
      const tpl = body.template_id ? TEMPLATES.find(t => t.id === body.template_id) ?? fail('TEMPLATE_NOT_FOUND', 400) : null;
      const d = mk({ title: body.title?.trim() || tpl?.name || '제목 없음', icon: tpl?.icon ?? body.icon ?? null, parent_id: parent?.id ?? null,
        staff_access: parent ? parent.staff_access : 'EDITOR', grants: parent ? structuredClone(parent.grants) : [], content: structuredClone(tpl?.content ?? { type: 'doc', content: [p()] }), template_id: tpl?.id ?? null }, 0);
      if (body.link?.entity_kind === 'task') {
        const t = ctx.tasks.find(x => x.id === body.link?.entity_id);
        if (t) t.links = [...t.links, { kind: 'doc', id: d.id, label: d.title, sub: d.status }];
      }
      return wait(detail(d));
    },
    saveDoc: (id: string, body: DocSaveInput) => {
      const d = find(id, 2);
      if (d.archived_at) fail('DOC_ARCHIVED');
      if (body.base_version !== d.version) fail('CONFLICT', 409);
      if (d.updated_by !== me || Date.now() - Date.parse(d.updated_at) > 10 * 60_000) snapshot(d, 'AUTOSAVE');
      if (body.title !== undefined) d.title = body.title.trim() || '제목 없음';
      if (body.icon !== undefined) d.icon = body.icon;
      if (body.content) d.content = body.content;
      if (d.status !== 'DRAFT') d.status = 'DRAFT';
      d.version += 1; d.updated_by = me; d.updated_at = new Date().toISOString();
      return wait({ id, version: d.version, status: d.status, saved_at: d.updated_at });
    },
    moveDoc: (id: string, body: { parent_id: string | null; before_id?: string | null; after_id?: string | null }) => {
      const d = find(id, 3);
      if (body.parent_id && subtree(id).some(x => x.id === body.parent_id)) fail('DOC_MOVE_CYCLE', 400);
      const parent = body.parent_id ? find(body.parent_id, 2) : null;
      const height = Math.max(...subtree(id).map(x => depth(x) - depth(d)));
      if (parent && depth(parent) + 1 + height > 4) fail('DOC_TOO_DEEP');
      d.parent_id = parent?.id ?? null;
      const a = docs.find(x => x.id === body.before_id)?.position;
      const b = docs.find(x => x.id === body.after_id)?.position;
      d.position = a != null && b != null ? (a + b) / 2 : a != null ? a + 1024 : b != null ? b - 1024 : Math.max(0, ...docs.map(x => x.position)) + 1024;
      return wait(detail(d));
    },
    duplicateDoc: (id: string) => {
      const s = find(id);
      const d = mk({ ...structuredClone(s), id: uid(), title: `${s.title} (사본)`, status: 'DRAFT', version: 1, created_by: me, updated_by: me, archived_at: null }, 0);
      return wait(detail(d));
    },
    archiveDoc: (id: string) => {
      find(id, 3);
      const at = new Date().toISOString();
      const ids = subtree(id).filter(x => !x.archived_at).map(x => { x.archived_at = at; return x.id; });
      return wait({ archived: ids });
    },
    restoreDoc: (id: string) => {
      const d = find(id, 3);
      for (const x of subtree(id)) x.archived_at = null;
      if (d.parent_id && docs.find(x => x.id === d.parent_id)?.archived_at) d.parent_id = null;
      return wait(detail(d));
    },
    deleteDoc: (id: string) => {
      const d = find(id, 3);
      if (!d.archived_at) fail('DOC_NOT_ARCHIVED');
      const ids = subtree(id).map(x => { x.deleted = true; return x.id; });
      return wait({ deleted: ids });
    },
    docStatus: (id: string, action: DocStatusAction, reason?: string) => {
      const d = find(id, 2);
      const now = new Date().toISOString();
      const ok = (cond: boolean) => { if (!cond) fail('DOC_STATUS_INVALID'); };
      if (action === 'REQUEST_REVIEW') { ok(d.status === 'DRAFT'); d.status = 'IN_REVIEW'; d.review = { ...d.review, requested_by: me, requested_at: now, approved_by: null, approved_at: null, rejected_by: null, rejected_at: null, rejection_reason: null }; }
      else if (action === 'APPROVE') { ok(d.status === 'IN_REVIEW'); d.status = 'APPROVED'; d.review = { ...d.review, approved_by: me, approved_at: now }; }
      else if (action === 'REJECT') { ok(d.status === 'IN_REVIEW'); if (!reason?.trim()) fail('REASON_REQUIRED', 400); d.status = 'DRAFT'; d.review = { ...d.review, rejected_by: me, rejected_at: now, rejection_reason: reason ?? null }; }
      else { ok(d.status !== 'DRAFT'); d.status = 'DRAFT'; }
      d.version += 1; d.updated_at = now; d.updated_by = me;
      snapshot(d, 'STATUS', action);
      return wait(detail(d));
    },
    docVersions: (id: string) => {
      const d = find(id);
      return wait({
        current: { version: d.version, updated_by: d.updated_by, updated_at: d.updated_at },
        items: versions.filter(v => v.doc === id).sort((a, b) => b.version - a.version || b.created_at.localeCompare(a.created_at))
          .map(v => ({ version: v.version, title: v.title, reason: v.reason, label: v.label, created_by: v.created_by, created_at: v.created_at, chars: plainText(v.content).length })),
      });
    },
    docVersion: (id: string, v: number) => {
      const d = find(id);
      if (v === d.version) return wait({ version: v, title: d.title, content: d.content, content_text: plainText(d.content), current: true });
      const x = versions.find(y => y.doc === id && y.version === v) ?? fail('NOT_FOUND', 404);
      return wait({ version: v, title: x.title, content: x.content, content_text: plainText(x.content), created_by: x.created_by, created_at: x.created_at, reason: x.reason, current: false });
    },
    saveDocVersion: (id: string, label?: string) => { const d = find(id, 2); snapshot(d, 'MANUAL', label ?? null); return api.docVersions(id); },
    restoreDocVersion: (id: string, version: number, base: number) => {
      const d = find(id, 2);
      if (base !== d.version) fail('CONFLICT', 409);
      const x = versions.find(y => y.doc === id && y.version === version) ?? fail('NOT_FOUND', 404);
      snapshot(d, 'RESTORE');
      d.content = structuredClone(x.content); d.title = x.title; d.version += 1; d.updated_at = new Date().toISOString(); d.updated_by = me; d.status = 'DRAFT';
      return wait(detail(d));
    },
    docPermissions: (id: string) => { const d = find(id, 3); return wait<DocPermissions>({ owner_id: d.created_by, staff_access: d.staff_access, grants: d.grants }); },
    setDocPermissions: (id: string, body) => {
      const d = find(id, 3);
      for (const x of body.apply_to_children ? subtree(id) : [d]) { x.staff_access = body.staff_access; x.grants = structuredClone(body.grants); }
      return wait<DocPermissions>({ owner_id: d.created_by, staff_access: d.staff_access, grants: d.grants });
    },
    docTemplates: () => wait({ items: TEMPLATES.map(({ content: _c, ...t }) => t) }),
    saveDocTemplate: (body: { from_doc: string; name: string; description: string }) => {
      const d = find(body.from_doc);
      TEMPLATES.push({ id: `tpl-${uid()}`, key: null, name: body.name, description: body.description, icon: d.icon, system: false, content: structuredClone(d.content) });
      return wait({ items: TEMPLATES.map(({ content: _c, ...t }) => t) });
    },
    archiveDocTemplate: (tplId: string) => {
      const i = TEMPLATES.findIndex(t => t.id === tplId && !t.system);
      if (i >= 0) TEMPLATES.splice(i, 1);
      return wait({ items: TEMPLATES.map(({ content: _c, ...t }) => t) });
    },
    docComments: (docId: string) => { find(docId); return wait({ items: comments.filter(c => c.target_id === docId) }); },
    addDocComment: body => {
      find(body.target_id);
      if (body.parent_id && !comments.some(c => c.id === body.parent_id && !c.parent_id)) fail('NOTE_PARENT_INVALID', 400);
      const c: DocComment = {
        id: uid(), target_kind: 'doc', target_id: body.target_id, author_id: me, body: body.body, mentions: body.mentions, attachments: [],
        resolved: false, resolved_at: null, resolved_by: null, edited_at: null, revisions: 0, created_at: new Date().toISOString(), archived_at: null,
        parent_id: body.parent_id ?? null, anchor: body.anchor ?? null,
      };
      comments.push(c);
      // 해결 표시는 메모 API(resolveNote)로 — 같은 저장소를 쓴다
      ctx.notes.push(c);
      return wait(c);
    },
    embeds: (refs: string[]) => {
      const items: Record<string, EmbedData> = {};
      for (const key of refs) {
        const [kind, id, view = 'summary'] = key.split(':');
        const base = { kind, id, view: view as EmbedData['view'] };
        if (kind === 'release') {
          const r = ctx.resolve('release', id);
          if (r.missing) { items[key] = { ...base, missing: true }; continue; }
          items[key] = {
            ...base, title: r.label, artist: '한결', upc: '8800000000011', release_date: '2026-10-15', status: 'IN_REVIEW', org_name: r.sub,
            qc: { checks: { total: 18, passed: 16, blocked: 0 }, issues: [{ code: 'AUDIO_LOUDNESS_OUT_OF_RANGE', status: 'REVIEW_REQUIRED', detail: 'integrated_lufs=-7.9 true_peak_dbtp=0.4' }], loudness: { min_lufs: -9.2, max_lufs: -7.9, max_true_peak: 0.4 }, audio: { sample_rates: [44100], channels: [2] } },
            rights: { agreement: 'SIGNED', pending_approvals: 1, open_checks: 2, claimed_by: null, rights_epoch: 3 },
            distribution: [
              { dsp: 'D-1', name: '멜론', delivery: 'DELIVERED', delivered_at: iso(3), live: 'LIVE', error: null },
              { dsp: 'D-2', name: '지니', delivery: 'FAILED', delivered_at: null, live: 'UNKNOWN', error: 'SFTP host key mismatch' },
            ],
          };
        } else if (kind === 'task') {
          const t = ctx.tasks.find(x => x.id === id);
          items[key] = t ? { ...base, seq: t.key.replace('WRK-', ''), title: t.title, priority: t.priority, status: t.status, column: t.column_name ?? null, board: null, assignee_id: t.assignee_id, due_at: t.due_at } : { ...base, missing: true };
        } else if (kind === 'doc') {
          const d = docs.find(x => x.id === id && !x.deleted);
          items[key] = d ? { ...base, title: d.title, icon: d.icon, status: d.status, updated_at: d.updated_at } : { ...base, missing: true };
        } else if (kind === 'audit') {
          items[key] = { ...base, events: [{ action: 'staff.release_decided', reason: 'APPROVE', at: iso(5), actor_id: others[0], service: null }], total: 1 };
        } else if (kind === 'dsp') {
          const r = ctx.resolve('dsp', id);
          items[key] = r.missing ? { ...base, missing: true } : { ...base, name: r.label, region: 'KR', status: 'ACTIVE', delivered_30d: 214, failed_30d: 3, in_flight: 2, last_delivered_at: iso(1), last_error: 'timeout' };
        } else if (kind === 'inquiry') {
          const r = ctx.resolve('inquiry', id);
          items[key] = r.missing ? { ...base, missing: true } : { ...base, subject: r.label, status: 'OPEN', category: 'RELEASE', org_name: 'Lumi Records', task: null };
        } else {
          const r = ctx.resolve(kind as EntityRef['kind'], id);
          items[key] = r.missing ? { ...base, missing: true } : { ...base, label: r.label, sub: r.sub };
        }
      }
      return wait({ items });
    },
    attachmentInline: () => '',
  };
  return api;
}
