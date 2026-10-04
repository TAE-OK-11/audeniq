// 체험(목) 빌드용 칸반 API — crates/core/src/work/kanban.rs 규칙을 흉내 낸다.
// 카드는 업무(ctx.tasks)와 같은 객체에 board_id·column_id·position·source를 붙여 둔다.
// 상태 규칙(검토자 → 검토 요청, 하위 업무, 반복)은 ctx.updateTask(목 업무 API)가 정하고,
// 상태가 바뀐 업무는 그 상태의 첫 열로 옮겨진다(서버 tasks::apply와 같게).
import { ApiError, messageForCode } from '../../api/errors';
import { GAP, columnCards, positionBetween } from './board/logic';
import type {
  AutoTrigger, Board, BoardInput, BoardQuery, BoardRule, BoardView, CardSource, ColumnInput, EntityRef, MoveInput,
  RuleInput, StaffRoleKey, TaskDetail, TaskInput, TaskPatch, TaskStatus, WorkNote, WorkTask,
} from './types';
import type { WorkApi } from './api';

export interface BoardsCtx {
  me: string;
  tasks: WorkTask[];
  notes: WorkNote[];
  resolve: (kind: EntityRef['kind'], id: string) => EntityRef;
  createTask: (i: TaskInput) => Promise<TaskDetail>;
  updateTask: (id: string, p: TaskPatch) => Promise<TaskDetail>;
}
type BoardApi = Pick<WorkApi, 'boards' | 'board' | 'createBoard' | 'updateBoard' | 'archiveBoard' | 'addColumn' | 'updateColumn'
  | 'archiveColumn' | 'orderColumns' | 'moveCard' | 'rules' | 'saveRule' | 'runAutomation'>;

const H = 3_600_000;
const wait = <T>(v: T) => new Promise<T>(r => setTimeout(() => r(structuredClone(v)), 140));
const fail = (code: string, status = 422, message?: string): never => { throw new ApiError(message ?? messageForCode(code, status), status, code); };
const iso = (hoursAgo: number) => new Date(Date.now() - hoursAgo * H).toISOString();
const uid = () => Math.random().toString(16).slice(2, 10);

const DEFAULT_COLUMNS: [string, TaskStatus][] = [
  ['Inbox', 'TODO'], ['To Do', 'TODO'], ['In Progress', 'IN_PROGRESS'], ['Review', 'IN_REVIEW'], ['Waiting', 'BLOCKED'], ['Blocked', 'BLOCKED'], ['Done', 'DONE'],
];
const TRIGGERS: AutoTrigger[] = [
  'QC_FAILURE', 'METADATA_CONFLICT', 'MISSING_RIGHTS_DOCUMENT', 'DSP_REJECTION', 'DISTRIBUTION_FAILURE',
  'SETTLEMENT_MISMATCH', 'AUTOMATION_FAILURE', 'SUPPORT_INQUIRY', 'APPROVAL_REQUIRED', 'RELEASE_REVIEW_OVERDUE',
];
const TRIGGER_SOURCE: Record<AutoTrigger, CardSource> = {
  QC_FAILURE: 'qc', METADATA_CONFLICT: 'qc', MISSING_RIGHTS_DOCUMENT: 'rights', DSP_REJECTION: 'distribution',
  DISTRIBUTION_FAILURE: 'distribution', SETTLEMENT_MISMATCH: 'finance', AUTOMATION_FAILURE: 'system', SUPPORT_INQUIRY: 'support',
  APPROVAL_REQUIRED: 'system', RELEASE_REVIEW_OVERDUE: 'sla',
};
const closedStatus = (s: TaskStatus) => s === 'DONE' || s === 'CANCELLED';

interface BoardRow { b: Omit<Board, 'access' | 'open_cards'>; position: number }
interface ColumnRow { id: string; board: string; name: string; status: TaskStatus; position: number; wip_limit: number | null; archived: boolean }
interface RuleRow { id: string; trigger: AutoTrigger; board: string; column: string; enabled: boolean; active_since: string }

export function makeBoards(ctx: BoardsCtx): BoardApi {
  const boards: BoardRow[] = [];
  const columns: ColumnRow[] = [];
  const rules: RuleRow[] = [];
  let autoSeq = 2001;

  const addBoard = (key: string | null, name: string, description: string, kind: Board['kind'], teamRole: StaffRoleKey | null, owner: string | null = null) => {
    const b: BoardRow['b'] = {
      id: `b-${key ?? uid()}`, key, name, description, kind, owner_id: owner, team_role: teamRole, created_by: key ? null : ctx.me, row_version: 1, archived: false,
    };
    boards.push({ b, position: boards.length });
    DEFAULT_COLUMNS.forEach(([n, status], i) => columns.push({ id: `col-${uid()}`, board: b.id, name: n, status, position: i, wip_limit: null, archived: false }));
    return b;
  };
  const general = addBoard('general', '일반 업무', '모든 업무가 처음 놓이는 기본 보드예요.', 'OPERATIONS', null);
  const distribution = addBoard('distribution', '배급 운영', 'DSP 전송·반려·장애 대응 카드를 모아요.', 'OPERATIONS', null);
  addBoard('rights', '권리 검토', '권리 서류·계약 검토 팀 보드예요.', 'TEAM', 'REVIEWER');
  const support = addBoard('support', '고객 지원', '문의 응대와 에스컬레이션.', 'TEAM', 'SUPPORT');
  addBoard('engineering', '엔지니어링', '자동화 실패·시스템 오류 처리.', 'TEAM', 'ADMIN');

  const colsOf = (board: string) => columns.filter(c => c.board === board && !c.archived).sort((a, b) => a.position - b.position);
  const colById = (id: string) => columns.find(c => c.id === id && !c.archived);
  const firstOf = (board: string, status: TaskStatus) => colsOf(board).find(c => c.status === status);
  const endOf = (col: string) => Math.max(0, ...ctx.tasks.filter(t => t.column_id === col && !t.archived_at).map(t => t.position ?? 0)) + GAP;
  const place = (t: WorkTask, col: ColumnRow) => {
    t.board_id = col.board; t.column_id = col.id; t.column_name = col.name; t.position = endOf(col.id);
  };
  // 너비 맞추기: 열 이름 표시 + 상태가 바뀐 업무(업무 상세에서 바꾼 것 등)는 그 상태의 첫 열로, 보드 없는 새 업무는 기본 보드로
  const sync = () => {
    for (const t of ctx.tasks) {
      if (t.parent_id || t.archived_at) continue;
      t.source ??= 'manual';
      const col = t.column_id ? colById(t.column_id) : undefined;
      if (!col || col.board !== t.board_id) {
        const board = boards.find(b => b.b.id === t.board_id && !b.b.archived)?.b.id ?? general.id;
        const target = firstOf(board, t.status) ?? colsOf(board).find(c => c.status === 'TODO') ?? colsOf(board)[0];
        if (target) place(t, target);
        continue;
      }
      t.column_name = col.name;
      if (t.position == null) t.position = endOf(col.id);
      if (col.status !== t.status) { const to = firstOf(col.board, t.status); if (to) place(t, to); }
    }
  };

  // 기존 업무(상위·보관 아님)는 일반 보드에 상태별로, 자동 카드 두 장
  const genCols = colsOf(general.id);
  const STATUS_COL: Partial<Record<TaskStatus, number>> = { TODO: 1, IN_PROGRESS: 2, IN_REVIEW: 3, BLOCKED: 5, DONE: 6, CANCELLED: 6 };
  for (const t of ctx.tasks) {
    if (t.parent_id || t.archived_at) continue;
    t.source = 'manual';
    t.board_id = general.id;
    place(t, genCols[STATUS_COL[t.status] ?? 1]);
  }
  const auto = (source: CardSource, board: string, column: ColumnRow, v: Partial<WorkTask> & Pick<WorkTask, 'title' | 'type'>, hoursAgo: number) => {
    const at = iso(hoursAgo);
    const t: WorkTask = {
      id: `t-${uid()}`, key: `WRK-${autoSeq++}`, parent_id: null, description: '', status: column.status, priority: 'HIGH',
      assignee_id: null, reviewer_id: null, due_at: null, overdue: false, changes_requested: false, recurrence: null, recurred_from: null,
      tags: [], created_by: null, created_at: at, updated_at: at, assigned_at: null, started_at: null, completed_at: null, archived_at: null,
      row_version: 1, subtasks: { total: 0, done: 0 }, note_count: 0, links: [], source, board_id: board, ...v,
    };
    place(t, column);
    ctx.tasks.push(t);
    return t;
  };
  auto('qc', general.id, genCols[0], {
    title: 'QC 실패 — 여름 끝에서 트랙 2 클리핑 감지', type: 'RELEASE', priority: 'URGENT', tags: ['QC'],
    description: '자동 검사에서 트랙 2의 트루 피크가 0 dBTP를 넘었어요. 아티스트에게 보완 요청이 필요한지 확인해 주세요.',
    links: [ctx.resolve('release', 'r3c4d5e6')], due_at: iso(-6),
  }, 0.8);
  auto('support', support.id, colsOf(support.id)[0], {
    title: '지급 계좌 변경 문의 — 24시간 미응답', type: 'SUPPORT', tags: ['문의'],
    links: [ctx.resolve('inquiry', 'iq-2')], due_at: iso(2),
  }, 26);
  auto('distribution', distribution.id, colsOf(distribution.id)[0], {
    title: 'Spotify 반려 — 피처링 아티스트 표기', type: 'DSP_INQUIRY', priority: 'NORMAL', links: [ctx.resolve('dsp', 'D-5')],
  }, 5);
  rules.push(
    { id: `r-${uid()}`, trigger: 'QC_FAILURE', board: general.id, column: genCols[0].id, enabled: true, active_since: iso(300) },
    { id: `r-${uid()}`, trigger: 'SUPPORT_INQUIRY', board: support.id, column: colsOf(support.id)[0].id, enabled: true, active_since: iso(300) },
    { id: `r-${uid()}`, trigger: 'DSP_REJECTION', board: distribution.id, column: colsOf(distribution.id)[0].id, enabled: false, active_since: iso(300) },
  );
  // 지금 실행하면 생길 예시 사건 (규칙이 켜진 것만 카드가 된다)
  const pending: { trigger: AutoTrigger; title: string; type: WorkTask['type'] }[] = [
    { trigger: 'QC_FAILURE', title: 'QC 실패 — Blue Hour 커버 해상도 부족', type: 'RELEASE' },
    { trigger: 'SUPPORT_INQUIRY', title: '새 문의 — 발매일 변경 가능 여부', type: 'SUPPORT' },
    { trigger: 'DSP_REJECTION', title: 'DSP 반려 — Apple Music 장르 코드', type: 'DSP_INQUIRY' },
    { trigger: 'SETTLEMENT_MISMATCH', title: '정산 불일치 — 9월 YouTube Music 차액', type: 'SETTLEMENT' },
  ];

  // ---------- 권한 (me는 체험에서 ADMIN) ----------
  const isAdmin = true;
  const canRead = (b: BoardRow['b']) => b.kind !== 'PERSONAL' || b.owner_id === ctx.me;
  const canWrite = (b: BoardRow['b']) => !b.archived && (b.kind === 'PERSONAL' ? b.owner_id === ctx.me : b.kind === 'TEAM' ? isAdmin : true);
  const canManage = (b: BoardRow['b']) => (b.kind === 'PERSONAL' ? b.owner_id === ctx.me : isAdmin || b.created_by === ctx.me);
  const json = (b: BoardRow['b']): Board => ({ ...b, access: { write: canWrite(b), manage: canManage(b) } });
  const readable = (id: string) => {
    const r = boards.find(x => x.b.id === id);
    if (!r || !canRead(r.b)) return fail('NOT_FOUND', 404);
    return r.b;
  };
  const managed = (id: string) => { const b = readable(id); if (!canManage(b)) fail('FORBIDDEN', 403); return b; };

  const notesOf = (t: WorkTask) => ctx.notes.filter(n => n.target_kind === 'task' && n.target_id === t.id && !n.archived_at).length;
  const view = (id: string, q: BoardQuery = {}): BoardView => {
    sync();
    const b = readable(id);
    const cols = colsOf(id);
    const term = q.q?.trim().toLowerCase();
    const doneSince = Date.now() - (q.done_days ?? 30) * 24 * H;
    const dayEnd = new Date().setHours(24, 0, 0, 0);
    const assignee = q.assignee === 'me' ? ctx.me : q.assignee;
    const cards = ctx.tasks.filter(t => {
      if (t.board_id !== id || t.archived_at || t.parent_id) return false;
      const col = t.column_id ? colById(t.column_id) : undefined;
      if (!col) return false;
      if (closedStatus(col.status) && Date.parse(t.completed_at ?? t.updated_at) < doneSince) return false;
      if (assignee === 'none' ? t.assignee_id : assignee && t.assignee_id !== assignee) return false;
      if (q.priority && t.priority !== q.priority) return false;
      if (q.column && t.column_id !== q.column) return false;
      if (q.tag && !t.tags.includes(q.tag)) return false;
      if (q.type && t.type !== q.type) return false;
      const due = t.due_at ? Date.parse(t.due_at) : null;
      if (q.due === 'overdue' && !(due != null && due < Date.now() && !closedStatus(col.status))) return false;
      if (q.due === 'today' && !(due != null && due < dayEnd)) return false;
      if (q.due === 'week' && !(due != null && due < Date.now() + 7 * 24 * H)) return false;
      if (q.due === 'none' && due != null) return false;
      if (q.creator && t.created_by !== q.creator) return false;
      if (q.source === 'auto' ? t.source === 'manual' : q.source && t.source !== q.source) return false;
      if (q.entity_kind && !t.links.some(l => l.kind === q.entity_kind)) return false;
      if (term && ![t.title, t.description, t.key, ...t.links.map(l => l.label ?? '')].some(v => v.toLowerCase().includes(term))) return false;
      return true;
    }).map(t => {
      t.note_count = notesOf(t);
      t.overdue = !!t.due_at && Date.parse(t.due_at) < Date.now() && !closedStatus(t.status);
      return t;
    });
    const ordered = cols.flatMap(c => columnCards(cards, c.id));
    return {
      board: json(b),
      columns: cols.map(c => ({
        id: c.id, name: c.name, status: c.status, position: c.position, wip_limit: c.wip_limit,
        count: ctx.tasks.filter(t => t.column_id === c.id && !t.archived_at).length,
      })),
      cards: ordered,
      tags: [...new Set(ctx.tasks.filter(t => t.board_id === id && !t.archived_at).flatMap(t => t.tags))].sort(),
      truncated: false,
    };
  };

  const rulesView = () => ({
    items: rules.map((r): BoardRule => {
      const b = boards.find(x => x.b.id === r.board)!.b;
      return {
        id: r.id, trigger: r.trigger, board_id: r.board, board_name: b.name, column_id: r.column, column_name: columns.find(c => c.id === r.column)?.name ?? '',
        enabled: r.enabled && !b.archived, active_since: r.active_since,
        cards: ctx.tasks.filter(t => t.board_id === r.board && t.source && t.source !== 'manual').length,
      };
    }).sort((a, b) => a.trigger.localeCompare(b.trigger)),
    triggers: TRIGGERS,
  });

  const name = (v: string | undefined, max: number) => {
    const n = (v ?? '').trim();
    if (!n) fail('NAME_REQUIRED', 400);
    if (n.length > max) fail('INVALID_INPUT', 400);
    return n;
  };
  const wip = (v: number | null | undefined) => {
    if (v != null && !(v >= 1 && v <= 999)) fail('WIP_LIMIT_RANGE', 400);
    return v ?? null;
  };

  return {
    boards: () => {
      sync();
      return wait({
        items: boards.filter(r => !r.b.archived && canRead(r.b))
          .sort((a, b) => Number(b.b.kind === 'PERSONAL') - Number(a.b.kind === 'PERSONAL') || a.position - b.position)
          .map(r => ({
            ...json(r.b),
            open_cards: ctx.tasks.filter(t => t.board_id === r.b.id && !t.archived_at && !t.parent_id && !closedStatus(colById(t.column_id ?? '')?.status ?? 'DONE')).length,
          })),
      });
    },
    board: (id: string, q: BoardQuery = {}) => wait(view(id, q)),
    createBoard: (i: BoardInput) => {
      const kind = i.kind ?? 'PERSONAL';
      if (kind !== 'PERSONAL' && !isAdmin) fail('FORBIDDEN', 403);
      if (kind === 'TEAM' && !i.team_role) fail('ROLE_REQUIRED', 400);
      const b = addBoard(null, name(i.name, 80), (i.description ?? '').trim(), kind, kind === 'TEAM' ? i.team_role ?? null : null, kind === 'PERSONAL' ? ctx.me : null);
      return wait(view(b.id));
    },
    updateBoard: (id: string, i: BoardInput) => {
      const b = managed(id);
      if (i.row_version != null && i.row_version !== b.row_version) fail('CONFLICT', 409);
      b.name = name(i.name, 80);
      b.description = (i.description ?? '').trim();
      if (b.kind === 'TEAM' && i.team_role) b.team_role = i.team_role;
      b.row_version++;
      return wait(view(id));
    },
    archiveBoard: (id: string) => {
      const b = managed(id);
      if (b.archived) fail('CONFLICT', 409);
      if (b.key === 'general') fail('BOARD_DEFAULT');
      b.archived = true;
      b.row_version++;
      for (const r of rules) if (r.board === id) r.enabled = false;
      return wait({ archived: id });
    },
    addColumn: (id: string, i: ColumnInput) => {
      managed(id);
      const n = name(i.name, 40);
      const cols = colsOf(id);
      if (cols.length >= 20) fail('TOO_MANY_COLUMNS');
      if (cols.some(c => c.name === n)) fail('COLUMN_EXISTS');
      columns.push({ id: `col-${uid()}`, board: id, name: n, status: i.status ?? 'TODO', position: Math.max(0, ...cols.map(c => c.position)) + 1, wip_limit: wip(i.wip_limit), archived: false });
      return wait(view(id));
    },
    updateColumn: (colId: string, i: ColumnInput) => {
      const c = colById(colId) ?? fail('NOT_FOUND', 404);
      managed(c.board);
      if (i.name !== undefined) {
        const n = name(i.name, 40);
        if (colsOf(c.board).some(x => x.id !== c.id && x.name === n)) fail('COLUMN_EXISTS');
        c.name = n;
      }
      if (i.wip_limit !== undefined) c.wip_limit = wip(i.wip_limit);
      if (i.status && i.status !== c.status) {
        c.status = i.status;
        const at = new Date().toISOString();
        // 열의 카드는 새 상태를 따른다
        for (const t of ctx.tasks.filter(x => x.column_id === c.id && !x.archived_at)) {
          t.status = c.status;
          if (c.status === 'IN_PROGRESS' || c.status === 'IN_REVIEW') t.started_at ??= at;
          t.completed_at = closedStatus(c.status) ? t.completed_at ?? at : null;
          t.updated_at = at; t.row_version++;
        }
      }
      return wait(view(c.board));
    },
    archiveColumn: (colId: string, moveTo?: string | null) => {
      const c = colById(colId) ?? fail('NOT_FOUND', 404);
      managed(c.board);
      if (colsOf(c.board).length <= 1) fail('BOARD_NEEDS_COLUMN');
      const cards = columnCards(ctx.tasks.filter(t => !t.archived_at), c.id);
      if (cards.length) {
        if (!moveTo) fail('MOVE_TO_REQUIRED', 400);
        const target = colById(moveTo ?? '');
        if (!target || target.id === c.id || target.board !== c.board) return fail('MOVE_TO_INVALID', 400);
        const at = new Date().toISOString();
        for (const t of cards) {
          place(t, target);
          t.status = target.status;
          t.completed_at = closedStatus(target.status) ? t.completed_at ?? at : null;
          t.updated_at = at; t.row_version++;
        }
      }
      c.archived = true;
      for (const r of rules) if (r.column === c.id) r.enabled = false;
      return wait(view(c.board));
    },
    orderColumns: (id: string, ids: string[]) => {
      managed(id);
      const current = colsOf(id).map(c => c.id).sort();
      if (JSON.stringify([...ids].sort()) !== JSON.stringify(current)) fail('COLUMN_ORDER_INVALID', 400);
      ids.forEach((cid, n) => { columns.find(c => c.id === cid)!.position = n; });
      return wait(view(id));
    },
    moveCard: async (id: string, i: MoveInput) => {
      sync();
      const t = ctx.tasks.find(x => x.id === id) ?? fail('NOT_FOUND', 404);
      if (t.archived_at) fail('TASK_ARCHIVED');
      if (t.row_version !== i.row_version) fail('CONFLICT', 409);
      if (!t.board_id) fail('TASK_NOT_ON_BOARD', 400);
      const b = readable(t.board_id ?? '');
      if (!canWrite(b) && t.assignee_id !== ctx.me && t.reviewer_id !== ctx.me) fail('FORBIDDEN', 403);
      const col = colById(i.column_id) ?? fail('NOT_FOUND', 404);
      if (col.board !== t.board_id) fail('COLUMN_NOT_ON_BOARD', 400);
      if (i.before_id === id || i.after_id === id) fail('NEIGHBOUR_INVALID', 400);
      const posOf = (n: string | null | undefined) => {
        if (!n) return null;
        const x = ctx.tasks.find(y => y.id === n && y.column_id === col.id && !y.archived_at);
        return x ? x.position ?? 0 : fail('NEIGHBOUR_NOT_IN_COLUMN', 400);
      };
      // 상태 먼저 (검토자·하위 업무 규칙) — 검토자가 있으면 '완료'는 검토 요청이 된다
      if (col.status !== t.status) {
        if (col.status === 'IN_REVIEW' && !t.reviewer_id) fail('TASK_NO_REVIEWER');
        await ctx.updateTask(id, { row_version: t.row_version, status: col.status });
      }
      if (t.status === col.status) {
        const a = posOf(i.before_id);
        const z = posOf(i.after_id);
        t.board_id = col.board; t.column_id = col.id; t.column_name = col.name;
        t.position = a == null && z == null ? endOf(col.id) : positionBetween(a, z);
      } else {
        // 검토 요청으로 돌려진 카드는 그 상태의 첫 열 끝으로
        const to = firstOf(col.board, t.status);
        if (to) place(t, to);
      }
      t.updated_at = new Date().toISOString();
      t.row_version++;
      return wait({ card: t, redirected: t.status !== col.status });
    },
    rules: () => wait(rulesView()),
    saveRule: (i: RuleInput) => {
      if (!isAdmin) fail('FORBIDDEN', 403);
      if (!TRIGGERS.includes(i.trigger)) fail('TRIGGER_UNKNOWN', 400);
      const b = boards.find(x => x.b.id === i.board_id)?.b;
      if (!b || b.archived || b.kind === 'PERSONAL') return fail('BOARD_INVALID', 400);
      const c = colById(i.column_id);
      if (!c || c.board !== b.id) return fail('COLUMN_NOT_ON_BOARD', 400);
      const r = rules.find(x => x.trigger === i.trigger);
      if (r) {
        if (i.enabled && !r.enabled) r.active_since = new Date().toISOString();
        Object.assign(r, { board: b.id, column: c.id, enabled: i.enabled });
      } else {
        rules.push({ id: `r-${uid()}`, trigger: i.trigger, board: b.id, column: c.id, enabled: i.enabled, active_since: new Date().toISOString() });
      }
      return wait(rulesView());
    },
    runAutomation: () => {
      if (!isAdmin) fail('FORBIDDEN', 403);
      let created = 0;
      for (let n = pending.length - 1; n >= 0; n--) {
        const ev = pending[n];
        const r = rules.find(x => x.trigger === ev.trigger && x.enabled);
        const col = r ? colById(r.column) : undefined;
        if (!r || !col || boards.find(x => x.b.id === r.board)?.b.archived) continue;
        auto(TRIGGER_SOURCE[ev.trigger], r.board, col, { title: ev.title, type: ev.type }, 0);
        pending.splice(n, 1);
        created++;
      }
      return wait({ created, at: new Date().toISOString() });
    },
  };
}
