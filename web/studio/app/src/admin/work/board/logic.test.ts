import { describe, expect, it } from 'vitest';
import { ApiError } from '../../../api/errors';
import {
  EMPTY_FILTERS, activeFilters, applyMove, boardError, columnCards, filtersFromParams, filtersToParams, groupCards, insertIndex, moveTarget,
  needsReload, positionBetween, shiftCount, toQuery, wipState,
} from './logic';
import type { BoardColumn, WorkTask } from '../types';

const col = (id: string, status: BoardColumn['status'] = 'TODO', count = 0): BoardColumn => ({ id, name: id.toUpperCase(), status, position: 0, wip_limit: null, count });
const card = (id: string, column: string, position: number): WorkTask => ({
  id, key: `WRK-${id}`, parent_id: null, title: id, description: '', type: 'GENERAL', status: 'TODO', priority: 'NORMAL',
  assignee_id: null, reviewer_id: null, due_at: null, overdue: false, changes_requested: false, recurrence: null, recurred_from: null,
  tags: [], created_by: null, created_at: '2026-10-01T00:00:00Z', updated_at: '2026-10-01T00:00:00Z', assigned_at: null, started_at: null,
  completed_at: null, archived_at: null, row_version: 1, subtasks: { total: 0, done: 0 }, note_count: 0, links: [],
  board_id: 'b', column_id: column, position,
});
const cols = [col('a'), col('b', 'IN_PROGRESS'), col('c', 'DONE')];
// a: a1(1024) a2(2048) a3(3072) · b: b1(1024) · c: 비어 있음
const cards = [card('a2', 'a', 2048), card('b1', 'b', 1024), card('a1', 'a', 1024), card('a3', 'a', 3072)];

describe('카드 순서', () => {
  it('열의 카드는 위치 순', () => {
    expect(columnCards(cards, 'a').map(c => c.id)).toEqual(['a1', 'a2', 'a3']);
  });
  it('이웃 사이 위치 — 서버 between과 같은 규칙', () => {
    expect(positionBetween(1024, 2048)).toBe(1536);
    expect(positionBetween(1024, null)).toBe(2048);
    expect(positionBetween(null, 1024)).toBe(0);
    expect(positionBetween(null, null)).toBe(1024);
  });
  it('묶음은 바뀌지 않은 열의 배열을 그대로 쓴다', () => {
    const g1 = groupCards(cards, cols);
    const moved = applyMove(cards, 'b1', cols[2], 0)!;
    const g2 = groupCards(moved.cards, cols, g1);
    expect(g2.get('a')).toBe(g1.get('a'));
    expect(g2.get('b')).not.toBe(g1.get('b'));
    expect(g2.get('c')!.map(c => c.id)).toEqual(['b1']);
  });
});

describe('applyMove', () => {
  it('다른 열 가운데로 — 이웃과 상태', () => {
    const r = applyMove(cards, 'b1', cols[0], 1)!;
    expect(r.before_id).toBe('a1');
    expect(r.after_id).toBe('a2');
    expect(r.card).toMatchObject({ column_id: 'a', status: 'TODO', position: 1536, column_name: 'A' });
    expect(columnCards(r.cards, 'a').map(c => c.id)).toEqual(['a1', 'b1', 'a2', 'a3']);
    // 원래 배열은 그대로
    expect(cards.find(c => c.id === 'b1')!.column_id).toBe('b');
  });
  it('빈 열·맨 앞·맨 끝', () => {
    expect(applyMove(cards, 'a1', cols[2], 0)).toMatchObject({ before_id: null, after_id: null, card: { status: 'DONE', position: 1024 } });
    expect(applyMove(cards, 'b1', cols[0], 0)).toMatchObject({ before_id: null, after_id: 'a1' });
    expect(applyMove(cards, 'b1', cols[0], 99)).toMatchObject({ before_id: 'a3', after_id: null, index: 3 });
  });
  it('같은 열 안에서 아래로 — 자리는 그 카드를 뺀 목록 기준', () => {
    const r = applyMove(cards, 'a1', cols[0], 1)!;
    expect([r.before_id, r.after_id]).toEqual(['a2', 'a3']);
    expect(columnCards(r.cards, 'a').map(c => c.id)).toEqual(['a2', 'a1', 'a3']);
  });
  it('제자리면 null', () => {
    expect(applyMove(cards, 'a2', cols[0], 1)).toBeNull();
    expect(applyMove(cards, 'nope', cols[0], 0)).toBeNull();
  });
});

describe('끌어 놓을 자리', () => {
  it('포인터가 처음으로 가운데보다 위인 카드 앞', () => {
    const mids = [100, 200, 300];
    expect(insertIndex(mids, 50)).toBe(0);
    expect(insertIndex(mids, 150)).toBe(1);
    expect(insertIndex(mids, 250)).toBe(2);
    expect(insertIndex(mids, 999)).toBe(3);
    expect(insertIndex([], 10)).toBe(0);
  });
});

describe('키보드 이동 대상', () => {
  it('위·아래는 한 칸, 옆 열은 같은 높이(끝을 넘지 않게)', () => {
    expect(moveTarget(cards, cols, 'a2', 'up')).toEqual({ columnId: 'a', index: 0 });
    expect(moveTarget(cards, cols, 'a2', 'down')).toEqual({ columnId: 'a', index: 2 });
    expect(moveTarget(cards, cols, 'a1', 'up')).toBeNull();
    expect(moveTarget(cards, cols, 'a3', 'down')).toBeNull();
    expect(moveTarget(cards, cols, 'a3', 'right')).toEqual({ columnId: 'b', index: 1 });
    expect(moveTarget(cards, cols, 'a1', 'left')).toBeNull();
    expect(moveTarget(cards, cols, 'b1', 'right')).toEqual({ columnId: 'c', index: 0 });
  });
  it('아래로 옮긴 결과가 실제로 한 칸 아래', () => {
    const t = moveTarget(cards, cols, 'a1', 'down')!;
    const r = applyMove(cards, 'a1', cols[0], t.index)!;
    expect(columnCards(r.cards, 'a').map(c => c.id)).toEqual(['a2', 'a1', 'a3']);
  });
});

describe('열 카드 수 · WIP', () => {
  it('옮김에 맞춰 수를 고친다', () => {
    const cs = [col('a', 'TODO', 3), col('b', 'TODO', 1)];
    expect(shiftCount(cs, 'a', 'b').map(c => c.count)).toEqual([2, 2]);
    expect(shiftCount(cs, 'a', 'a')).toBe(cs);
  });
  it('WIP 상태', () => {
    expect(wipState({ count: 3, wip_limit: null })).toBeNull();
    expect(wipState({ count: 2, wip_limit: 3 })).toBe('ok');
    expect(wipState({ count: 3, wip_limit: 3 })).toBe('full');
    expect(wipState({ count: 4, wip_limit: 3 })).toBe('over');
  });
});

describe('필터 ↔ 주소', () => {
  it('주소에서 읽고, 모르는 값은 버린다', () => {
    const f = filtersFromParams(new URLSearchParams('assignee=me&priority=URGENT&due=week&source=qc&entity=release&q=%20멜론&done=90&type=NOPE&card=t1'));
    expect(f).toMatchObject({ assignee: 'me', priority: 'URGENT', due: 'week', source: 'qc', entity_kind: 'release', q: ' 멜론', done_days: 90, type: '' });
    expect(filtersFromParams(new URLSearchParams('priority=P0&done=3&assignee=<x>'))).toEqual(EMPTY_FILTERS);
  });
  it('주소로 쓸 때 card 같은 다른 값은 남긴다', () => {
    const base = new URLSearchParams('card=t1&priority=LOW');
    const out = filtersToParams({ ...EMPTY_FILTERS, assignee: 'none', tag: '긴급' }, base);
    expect(out.get('card')).toBe('t1');
    expect(out.get('priority')).toBeNull();
    expect(out.get('assignee')).toBe('none');
    expect(filtersFromParams(out)).toEqual({ ...EMPTY_FILTERS, assignee: 'none', tag: '긴급' });
  });
  it('API 조건과 켜진 필터', () => {
    const f = { ...EMPTY_FILTERS, assignee: 'me', q: '  ', source: 'auto' as const, done_days: 7 };
    expect(toQuery(f)).toEqual({ assignee: 'me', source: 'auto', done_days: 7 });
    expect(activeFilters(f)).toEqual(['assignee', 'source']);
    expect(activeFilters({ ...f, q: '멜론' })).toEqual(['assignee', 'source', 'q']);
    expect(activeFilters(EMPTY_FILTERS)).toEqual([]);
  });
});

describe('오류', () => {
  it('검토자 없음·충돌은 알아듣게, 충돌은 다시 불러온다', () => {
    const noReviewer = new ApiError('x', 422, 'TASK_NO_REVIEWER');
    expect(boardError(noReviewer, 'f')).toMatch(/검토자/);
    expect(needsReload(noReviewer)).toBe(false);
    const conflict = new ApiError('x', 409, 'CONFLICT');
    expect(needsReload(conflict)).toBe(true);
    expect(boardError(new Error('서버 문구'), 'f')).toBe('서버 문구');
    expect(boardError(null, '기본')).toBe('기본');
  });
});
