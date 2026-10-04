// 칸반 보드 화면 — 필터가 주소·API 조건으로 가는지, 빠른 추가, 옮기기 실패 시 정확히 되돌리는지.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { BrowserRouter, Route, Routes } from '../../../lib/router';
import { ToastProvider } from '../../../components/Toast';
import { ConfirmProvider } from '../../../components/Confirm';
import { ApiError } from '../../../api/errors';
import { StaffContext } from '../../ui';
import type { BoardQuery, BoardView, MoveInput, TaskInput, WorkTask } from '../types';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const api = vi.hoisted(() => ({
  people: vi.fn(),
  boards: vi.fn(),
  board: vi.fn(),
  createTask: vi.fn(),
  moveCard: vi.fn(),
  task: vi.fn(),
}));
vi.mock('../api', () => ({ workApi: api }));

import { Board } from '../pages/Board';

const card = (id: string, column: string, position: number, extra: Partial<WorkTask> = {}): WorkTask => ({
  id, key: `WRK-${id}`, parent_id: null, title: `카드 ${id}`, description: '', type: 'GENERAL', status: 'TODO', priority: 'NORMAL',
  assignee_id: null, reviewer_id: null, due_at: null, overdue: false, changes_requested: false, recurrence: null, recurred_from: null,
  tags: [], created_by: 'me', created_at: '2026-10-01T00:00:00Z', updated_at: '2026-10-01T00:00:00Z', assigned_at: null, started_at: null,
  completed_at: null, archived_at: null, row_version: 3, subtasks: { total: 0, done: 0 }, note_count: 0, links: [],
  board_id: 'b1', column_id: column, position, source: 'manual', ...extra,
});

function boardView(cards: WorkTask[]): BoardView {
  return {
    board: { id: 'b1', key: 'general', name: '일반 업무', description: '', kind: 'OPERATIONS', owner_id: null, team_role: null, created_by: null, row_version: 1, archived: false, access: { write: true, manage: true } },
    columns: [
      { id: 'todo', name: 'To Do', status: 'TODO', position: 0, wip_limit: 2, count: cards.filter(c => c.column_id === 'todo').length },
      { id: 'review', name: 'Review', status: 'IN_REVIEW', position: 1, wip_limit: null, count: cards.filter(c => c.column_id === 'review').length },
    ],
    cards,
    tags: [],
    truncated: false,
  };
}

let host: HTMLDivElement;
let root: Root;
const flush = () => act(async () => { await new Promise(r => setTimeout(r, 0)); });

async function mount(path = '/admin/board/b1') {
  window.history.replaceState(null, '', path);
  host = document.createElement('div');
  host.id = 'root';
  document.body.append(host);
  root = createRoot(host);
  const staff = { me: { user_id: 'me', role: 'ADMIN' as const, duties: [] }, counts: null, work: null, refreshCounts: () => {}, can: () => true };
  await act(async () => {
    root.render(
      <BrowserRouter>
        <ToastProvider>
          <ConfirmProvider>
            <StaffContext.Provider value={staff}>
              <Routes>
                <Route path="/admin/board/:id" element={<Board />} />
              </Routes>
            </StaffContext.Provider>
          </ConfirmProvider>
        </ToastProvider>
      </BrowserRouter>,
    );
  });
  await flush();
}

const column = (id: string) => host.querySelector<HTMLElement>(`#kbcol-${id}`)!;
const cardIds = (col: string) => [...column(col).querySelectorAll<HTMLElement>('[data-card-id]')].map(el => el.dataset.cardId);
function setValue(el: HTMLInputElement | HTMLSelectElement, v: string) {
  const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(proto, 'value')!.set!.call(el, v);
  el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true }));
}

beforeEach(() => {
  const cards = [card('a', 'todo', 1024), card('b', 'todo', 2048)];
  api.people.mockResolvedValue({ items: [{ user_id: 'me', email: 'me@audeniq.com', name: 'me', role: 'ADMIN' }] });
  api.boards.mockResolvedValue({ items: [{ ...boardView(cards).board, open_cards: 2 }] });
  api.board.mockImplementation(async (_id: string, q: BoardQuery = {}) => boardView(cards.filter(c => !q.priority || c.priority === q.priority)));
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  document.querySelectorAll('.aq-toast-stack').forEach(n => n.remove());
  vi.clearAllMocks();
});

describe('칸반 보드 화면', () => {
  it('열과 카드를 그리고 WIP 한도를 글자로 보여 준다', async () => {
    await mount();
    expect(cardIds('todo')).toEqual(['a', 'b']);
    expect(column('todo').textContent).toContain('WIP 2/2 · 한도 도달');
    // 검토 열에는 빠른 추가가 없다 (검토자가 있어야 해서)
    expect(column('review').querySelector('.adm-kb-addbtn')).toBeNull();
  });

  it('필터를 고르면 주소와 API 조건이 바뀐다', async () => {
    await mount();
    const pri = host.querySelector<HTMLSelectElement>('select[aria-label="우선순위"]')!;
    await act(async () => { setValue(pri, 'URGENT'); });
    await flush();
    expect(new URLSearchParams(window.location.search).get('priority')).toBe('URGENT');
    expect(api.board).toHaveBeenLastCalledWith('b1', { priority: 'URGENT' });
    expect(cardIds('todo')).toEqual([]);
    expect(host.textContent).toContain('조건에 맞는 카드가 없어요');
    // 켜진 필터 칩으로 끈다
    const chip = [...host.querySelectorAll<HTMLButtonElement>('.adm-kb-chip')].find(b => b.textContent?.includes('우선순위'))!;
    await act(async () => { chip.click(); });
    await flush();
    expect(window.location.search).toBe('');
    expect(cardIds('todo')).toEqual(['a', 'b']);
  });

  it('빠른 추가 — 이 보드·열로 만들고 열 끝에 붙인다', async () => {
    api.createTask.mockImplementation(async (i: TaskInput) => ({ task: card('n', i.column_id ?? '', 3072, { title: i.title }), parent: null, subtasks: [], events: [] }));
    await mount();
    await act(async () => { column('todo').querySelector<HTMLButtonElement>('.adm-kb-addbtn')!.click(); });
    const input = column('todo').querySelector<HTMLInputElement>('.adm-kb-add input')!;
    await act(async () => { setValue(input, '새 카드 제목'); });
    await act(async () => { column('todo').querySelector<HTMLButtonElement>('.adm-kb-add button[type=submit]')!.click(); });
    await flush();
    expect(api.createTask).toHaveBeenCalledWith({ title: '새 카드 제목', type: 'GENERAL', priority: 'NORMAL', board_id: 'b1', column_id: 'todo' });
    expect(cardIds('todo')).toEqual(['a', 'b', 'n']);
    expect(input.value).toBe('');
  });

  it('옮기기 실패 — 먼저 옮겨 보여 주고, 실패하면 정확히 되돌리고 알린다', async () => {
    let reject: (e: unknown) => void = () => {};
    api.moveCard.mockImplementation(() => new Promise((_, rj) => { reject = rj; }));
    await mount();
    const btn = host.querySelector<HTMLButtonElement>('[data-card-focus="b"]')!;
    await act(async () => { btn.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', altKey: true, bubbles: true })); });
    // 낙관적 이동: 검토 열로
    expect(cardIds('review')).toEqual(['b']);
    expect(cardIds('todo')).toEqual(['a']);
    expect(api.moveCard).toHaveBeenCalledWith('b', { row_version: 3, column_id: 'review', before_id: null, after_id: null } satisfies MoveInput);
    // 같은 카드는 옮기는 동안 다시 옮기지 않는다
    await act(async () => { host.querySelector<HTMLButtonElement>('[data-card-focus="b"]')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', altKey: true, bubbles: true })); });
    expect(api.moveCard).toHaveBeenCalledTimes(1);
    await act(async () => { reject(new ApiError('x', 422, 'TASK_NO_REVIEWER')); });
    await flush();
    expect(cardIds('todo')).toEqual(['a', 'b']);
    expect(cardIds('review')).toEqual([]);
    expect(document.body.textContent).toContain('먼저 카드에 검토자를 정해 주세요');
    // 409가 아니면 다시 불러오지 않는다
    expect(api.board).toHaveBeenCalledTimes(1);
  });

  it('같은 열 안에서 아래로 — 이웃 id를 보내고 서버 카드로 바꾼다', async () => {
    api.moveCard.mockImplementation(async (id: string, i: MoveInput) => ({ card: card(id, i.column_id, 3072, { row_version: 4 }), redirected: false }));
    await mount();
    await act(async () => { host.querySelector<HTMLButtonElement>('[data-card-focus="a"]')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', altKey: true, bubbles: true })); });
    await flush();
    expect(api.moveCard).toHaveBeenCalledWith('a', { row_version: 3, column_id: 'todo', before_id: 'b', after_id: null });
    expect(cardIds('todo')).toEqual(['b', 'a']);
  });
});
