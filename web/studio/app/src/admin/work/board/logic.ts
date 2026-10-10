// 칸반 보드 순수 로직 — 카드 끼워 넣기(이웃 계산), 끌어 놓을 자리, 키보드 이동 대상, 필터 ↔ 주소 검색어.
// 화면(Board.tsx)과 체험 API(mockBoards.ts)가 같이 쓰고, 서버 규칙(crates/core/src/work/kanban.rs)과 맞춘다:
// 위치는 이웃 사이 중간값(끝이면 ±1024), 열의 상태가 곧 카드 상태.
import { ApiError, errorMessage } from '../../../api/errors';
import { ENTITY_KIND, PRIORITIES, WORK_TYPES } from '../labels';
import type { BoardColumn, BoardQuery, CardSource, EntityKind, Priority, WorkTask, WorkType } from '../types';

export const GAP = 1024;

// ---------- 열 안의 순서 ----------
const byPlace = (a: WorkTask, b: WorkTask) =>
  (a.position ?? Number.MAX_SAFE_INTEGER) - (b.position ?? Number.MAX_SAFE_INTEGER) || a.created_at.localeCompare(b.created_at);

/** 한 열의 카드 (위치 순) */
export function columnCards(cards: WorkTask[], columnId: string): WorkTask[] {
  return cards.filter(c => c.column_id === columnId).sort(byPlace);
}

/**
 * 열별로 묶는다. 이전 묶음과 내용(카드 객체)이 같은 열은 이전 배열을 그대로 돌려줘
 * memo된 열이 다시 그려지지 않게 한다.
 */
export function groupCards(cards: WorkTask[], columns: Pick<BoardColumn, 'id'>[], prev?: Map<string, WorkTask[]>): Map<string, WorkTask[]> {
  const out = new Map<string, WorkTask[]>();
  for (const col of columns) out.set(col.id, []);
  for (const c of cards) if (c.column_id) out.get(c.column_id)?.push(c);
  for (const [id, list] of out) {
    list.sort(byPlace);
    const old = prev?.get(id);
    if (old && old.length === list.length && old.every((c, i) => c === list[i])) out.set(id, old);
  }
  return out;
}

/** 이웃 위치로 새 위치 — 서버 `between`과 같은 규칙 (빈틈이 없으면 서버가 열을 다시 번호 매긴다) */
export function positionBetween(before: number | null | undefined, after: number | null | undefined): number {
  if (before != null && after != null) return (before + after) / 2;
  if (before != null) return before + GAP;
  if (after != null) return after - GAP;
  return GAP;
}

export interface MoveResult {
  /** 옮긴 뒤의 카드 목록 (옮긴 카드만 새 객체) */
  cards: WorkTask[];
  card: WorkTask;
  /** 바로 위 카드 / 바로 아래 카드 — 서버 move의 before_id / after_id */
  before_id: string | null;
  after_id: string | null;
  /** 옮긴 열 안에서의 자리 (0부터) */
  index: number;
}

/**
 * 카드를 `toColumn`의 `index` 자리(그 카드를 뺀 목록 기준)로 옮긴다. 제자리면 null.
 * 상태는 열의 상태를 따른다 — 서버가 검토자 규칙으로 다른 열로 돌릴 수 있어 응답 카드로 다시 맞춘다.
 */
export function applyMove(
  cards: WorkTask[], cardId: string, toColumn: Pick<BoardColumn, 'id' | 'status' | 'name'>, index: number,
): MoveResult | null {
  const card = cards.find(c => c.id === cardId);
  if (!card) return null;
  const list = columnCards(cards, toColumn.id).filter(c => c.id !== cardId);
  const at = Math.max(0, Math.min(Math.trunc(index), list.length));
  const before = list[at - 1] ?? null;
  const after = list[at] ?? null;
  if (card.column_id === toColumn.id) {
    const now = columnCards(cards, toColumn.id);
    const i = now.findIndex(c => c.id === cardId);
    if ((now[i - 1]?.id ?? null) === (before?.id ?? null) && (now[i + 1]?.id ?? null) === (after?.id ?? null)) return null;
  }
  const moved: WorkTask = {
    ...card,
    column_id: toColumn.id,
    column_name: toColumn.name,
    status: toColumn.status,
    position: positionBetween(before?.position, after?.position),
  };
  return {
    cards: cards.map(c => (c.id === cardId ? moved : c)),
    card: moved,
    before_id: before?.id ?? null,
    after_id: after?.id ?? null,
    index: at,
  };
}

/** 끌어 놓을 자리 — 포인터 Y가 처음으로 카드 가운데보다 위인 곳 (끌고 있는 카드는 빼고 잰 가운데들) */
export function insertIndex(midpoints: number[], y: number): number {
  const i = midpoints.findIndex(m => y < m);
  return i < 0 ? midpoints.length : i;
}

export type MoveDir = 'left' | 'right' | 'up' | 'down';

/** 키보드 이동(Alt+화살표) 대상 — 옆 열은 같은 높이 자리, 위·아래는 한 칸. 갈 곳이 없으면 null */
export function moveTarget(
  cards: WorkTask[], columns: Pick<BoardColumn, 'id'>[], cardId: string, dir: MoveDir,
): { columnId: string; index: number } | null {
  const card = cards.find(c => c.id === cardId);
  if (!card?.column_id) return null;
  const ci = columns.findIndex(c => c.id === card.column_id);
  if (ci < 0) return null;
  const here = columnCards(cards, card.column_id);
  const i = here.findIndex(c => c.id === cardId);
  if (dir === 'up') return i > 0 ? { columnId: card.column_id, index: i - 1 } : null;
  if (dir === 'down') return i < here.length - 1 ? { columnId: card.column_id, index: i + 1 } : null;
  const next = columns[ci + (dir === 'left' ? -1 : 1)];
  if (!next) return null;
  return { columnId: next.id, index: Math.min(i, columnCards(cards, next.id).length) };
}

/** 열의 카드 수(WIP 판단용, 서버 count)를 옮김에 맞춰 고친다 */
export function shiftCount<C extends Pick<BoardColumn, 'id' | 'count'>>(columns: C[], from: string | null | undefined, to: string | null | undefined): C[] {
  if (!from || !to || from === to) return columns;
  return columns.map(c => (c.id === from ? { ...c, count: Math.max(0, c.count - 1) } : c.id === to ? { ...c, count: c.count + 1 } : c));
}

/** WIP 한도 상태 — 색만이 아니라 글자로도 보여 준다 */
export function wipState(col: Pick<BoardColumn, 'count' | 'wip_limit'>): 'over' | 'full' | 'ok' | null {
  if (!col.wip_limit) return null;
  return col.count > col.wip_limit ? 'over' : col.count === col.wip_limit ? 'full' : 'ok';
}

// ---------- 필터 ↔ 주소 검색어 ----------
export type DueFilter = '' | 'overdue' | 'today' | 'week' | 'none';
export interface BoardFilters {
  /** uuid · 'me' · 'none' */
  assignee: string;
  priority: Priority | '';
  column: string;
  tag: string;
  type: WorkType | '';
  due: DueFilter;
  creator: string;
  /** 'manual' · 'auto' · 출처 하나 */
  source: '' | 'manual' | 'auto' | CardSource;
  entity_kind: EntityKind | '';
  q: string;
  /** 완료 카드를 며칠치 볼지 (없으면 서버 기본 30일) */
  done_days: number | null;
}
export const EMPTY_FILTERS: BoardFilters = {
  assignee: '', priority: '', column: '', tag: '', type: '', due: '', creator: '', source: '', entity_kind: '', q: '', done_days: null,
};
export const FILTER_KEYS = ['assignee', 'priority', 'column', 'tag', 'type', 'due', 'creator', 'source', 'entity_kind', 'q', 'done_days'] as const;
export type FilterKey = typeof FILTER_KEYS[number];

export const DUE_OPTIONS: DueFilter[] = ['overdue', 'today', 'week', 'none'];
export const SOURCES: CardSource[] = ['system', 'qc', 'distribution', 'support', 'rights', 'finance', 'sla'];
export const DONE_DAYS = [7, 30, 90, 365];
/** 주소에서는 짧게 */
const PARAM: Record<FilterKey, string> = {
  assignee: 'assignee', priority: 'priority', column: 'column', tag: 'tag', type: 'type', due: 'due', creator: 'creator',
  source: 'source', entity_kind: 'entity', q: 'q', done_days: 'done',
};
const ID_RE = /^[\w-]{1,64}$/;

const pick = <T extends string>(v: string | null, allowed: readonly T[]): T | '' => (v && (allowed as readonly string[]).includes(v) ? (v as T) : '');

/** 주소 검색어 → 필터 (모르는 값은 버린다 — 서버도 다시 검사한다) */
export function filtersFromParams(p: URLSearchParams): BoardFilters {
  const get = (k: FilterKey) => p.get(PARAM[k]);
  const id = (v: string | null) => (v && ID_RE.test(v) ? v : '');
  const done = Number(get('done_days'));
  return {
    assignee: id(get('assignee')),
    priority: pick(get('priority'), PRIORITIES),
    column: id(get('column')),
    tag: (get('tag') ?? '').trim().slice(0, 30),
    type: pick(get('type'), WORK_TYPES),
    due: pick(get('due'), DUE_OPTIONS),
    creator: id(get('creator')),
    source: pick(get('source'), ['manual', 'auto', ...SOURCES] as const),
    entity_kind: pick(get('entity_kind'), Object.keys(ENTITY_KIND) as EntityKind[]),
    q: (get('q') ?? '').slice(0, 80),
    done_days: DONE_DAYS.includes(done) ? done : null,
  };
}

/** 필터 → 주소 검색어. 필터가 아닌 값(card 등)은 `base`에서 그대로 둔다 */
export function filtersToParams(f: BoardFilters, base?: URLSearchParams): URLSearchParams {
  const out = new URLSearchParams(base);
  for (const k of FILTER_KEYS) {
    const v = f[k];
    if (v == null || v === '') out.delete(PARAM[k]);
    else out.set(PARAM[k], String(v));
  }
  return out;
}

/** 필터 → 보드 API 검색 조건 */
export function toQuery(f: BoardFilters): BoardQuery {
  const q: BoardQuery = {};
  if (f.assignee) q.assignee = f.assignee;
  if (f.priority) q.priority = f.priority;
  if (f.column) q.column = f.column;
  if (f.tag) q.tag = f.tag;
  if (f.type) q.type = f.type;
  if (f.due) q.due = f.due;
  if (f.creator) q.creator = f.creator;
  if (f.source) q.source = f.source;
  if (f.entity_kind) q.entity_kind = f.entity_kind;
  if (f.q.trim()) q.q = f.q.trim();
  if (f.done_days) q.done_days = f.done_days;
  return q;
}

/** 켜진 필터 (완료 기간은 보기 설정이라 빼고 센다) */
export function activeFilters(f: BoardFilters): FilterKey[] {
  return FILTER_KEYS.filter(k => (k === 'q' ? f.q.trim() !== '' : k !== 'done_days' && f[k] !== '' && f[k] != null));
}

/** 카드 버튼 선택자 (포커스 되돌리기) */
export const cardSelector = (id: string) => `[data-card-focus="${id.replace(/["\\]/g, '\\$&')}"]`;

// ---------- 오류 문구 ----------
const BOARD_ERRORS: Record<string, string> = {
  TASK_NO_REVIEWER: '검토 열로 옮기려면 먼저 카드에 검토자를 정해 주세요.',
  TASK_SUBTASKS_OPEN: '하위 업무가 모두 끝나야 완료 열로 옮길 수 있어요.',
  TASK_NOT_IN_REVIEW: '검토 중인 카드가 아니에요. 보드를 새로 불러왔어요.',
  TASK_ARCHIVED: '보관된 카드예요. 보드를 새로 불러왔어요.',
  CONFLICT: '다른 곳에서 먼저 바뀐 카드예요. 보드를 새로 불러왔어요.',
  NEIGHBOUR_NOT_IN_COLUMN: '그 사이 열 순서가 바뀌었어요. 보드를 새로 불러왔어요.',
  COLUMN_NOT_ON_BOARD: '그 사이 열이 바뀌었어요. 보드를 새로 불러왔어요.',
  FORBIDDEN: '이 보드에서 이 작업을 할 권한이 없어요.',
  BOARD_ARCHIVED: '보관된 보드라 바꿀 수 없어요.',
  BOARD_DEFAULT: '기본 보드는 보관할 수 없어요.',
  BOARD_NEEDS_COLUMN: '보드에는 열이 하나 이상 있어야 해요.',
  TOO_MANY_COLUMNS: '열은 20개까지 만들 수 있어요.',
  TOO_MANY_BOARDS: '보드가 너무 많아요. 쓰지 않는 보드를 보관해 주세요.',
  COLUMN_EXISTS: '같은 이름의 열이 이미 있어요.',
  MOVE_TO_REQUIRED: '카드가 있는 열은 카드를 옮길 열을 골라야 보관할 수 있어요.',
  MOVE_TO_INVALID: '카드를 옮길 열을 다시 골라 주세요.',
  WIP_LIMIT_RANGE: 'WIP 한도는 1~999 사이로 정해 주세요.',
  NAME_REQUIRED: '이름을 입력해 주세요.',
  ROLE_REQUIRED: '팀 보드는 팀(역할)을 골라야 해요.',
  BOARD_INVALID: '개인 보드나 보관된 보드에는 자동 규칙을 둘 수 없어요.',
  TITLE_REQUIRED: '제목을 입력해 주세요.',
};

export const errorCode = (e: unknown) => (e instanceof ApiError ? e.code || (e.status === 409 ? 'CONFLICT' : '') : '');

/** 보드 작업 오류 → 사람이 읽는 말 */
export function boardError(e: unknown, fallback: string): string {
  return BOARD_ERRORS[errorCode(e)] ?? errorMessage(e, fallback);
}

/** 이 오류 뒤에는 보드를 다시 불러와야 한다 (낡은 row_version·이웃·열) */
export function needsReload(e: unknown): boolean {
  const code = errorCode(e);
  return (e instanceof ApiError && (e.status === 409 || e.status === 404))
    || ['CONFLICT', 'NEIGHBOUR_NOT_IN_COLUMN', 'COLUMN_NOT_ON_BOARD', 'TASK_ARCHIVED', 'TASK_NOT_IN_REVIEW', 'NOT_FOUND'].includes(code);
}
