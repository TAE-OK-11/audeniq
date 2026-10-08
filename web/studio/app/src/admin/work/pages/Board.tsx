// 칸반 보드 (/admin/board, /admin/board/:id) — 개인·팀·운영 보드의 열과 카드(업무).
// 카드를 끌어 놓거나(같은 열 안의 순서까지) 키보드(Alt+화살표·'옮기기')로 옮기면 화면을 먼저 바꾸고
// 서버(move)에 이웃 카드(before_id·after_id)를 보낸다. 실패하면 옮기기 전 카드로 정확히 되돌린다.
// 상태는 열이 정하고, 검토자 규칙으로 서버가 다른 열에 놓으면 응답 카드를 따른다.
// 권한은 서버가 판단하고, 화면은 access(write·manage) 플래그를 따라 버튼과 끌기를 켜고 끌 뿐이다.
import { useCallback, useEffect, useMemo, useRef, useState, type DragEvent } from 'react';
import { useNavigate, useParams, useSearchParams } from '../../../lib/router';
import { Modal } from '../../../components/Modal';
import { useToast } from '../../../components/Toast';
import { useConfirm } from '../../../components/Confirm';
import { errorMessage } from '../../../api/errors';
import { Glyph } from '../../../components/Glyph';
import { readJSON, writeJSON } from '../../../lib/storage';
import { Badge, EmptyState, ErrorBox, PageHeader, SkeletonList, WorkTabs, useStaff } from '../../ui';
import { workApi } from '../api';
import { BOARD_KIND, ROLE_NAME } from '../labels';
import { usePeople } from '../ui';
import type { Board as BoardInfo, BoardView, WorkTask } from '../types';
import type { CardHandlers, ColumnOption } from '../board/Card';
import { BoardColumnView, type DragInfo } from '../board/Column';
import { CardDrawer } from '../board/CardDrawer';
import { FilterBar } from '../board/FilterBar';
import { BoardCreateForm, ColumnManager, NewCardForm, RulesPanel } from '../board/Modals';
import {
  EMPTY_FILTERS, activeFilters, applyMove, boardError, cardSelector, filtersFromParams, filtersToParams, groupCards, moveTarget, needsReload, shiftCount, toQuery,
  type BoardFilters, type MoveDir,
} from '../board/logic';

const LAST_BOARD = 'adm.board.last';
const KIND_ORDER = ['PERSONAL', 'TEAM', 'OPERATIONS'] as const;

/** 처음 열 보드 — 지난번에 본 보드, 없으면 기본(general), 없으면 첫 보드 */
function defaultBoard(items: BoardInfo[]): string | null {
  const last = readJSON<string | null>(LAST_BOARD, null);
  return (items.find(b => b.id === last) ?? items.find(b => b.key === 'general') ?? items[0])?.id ?? null;
}

export function Board() {
  const { id: routeId } = useParams<{ id: string }>();
  const nav = useNavigate();
  const [params, setParams] = useSearchParams();
  const toast = useToast();
  const confirm = useConfirm();
  const { me } = useStaff();
  const people = usePeople();
  const admin = me.role === 'ADMIN';

  const filters = useMemo(() => filtersFromParams(params), [params]);
  const queryKey = JSON.stringify(toQuery(filters));
  const openCard = params.get('card');

  // ---------- 보드 목록 ----------
  const [boards, setBoards] = useState<BoardInfo[] | null>(null);
  const [boardsError, setBoardsError] = useState('');
  const loadBoards = useCallback(() => {
    setBoardsError('');
    workApi.boards().then(r => setBoards(r.items)).catch(e => setBoardsError(errorMessage(e, '보드 목록을 불러오지 못했어요.')));
  }, []);
  useEffect(loadBoards, [loadBoards]);
  // 주소에 보드가 없으면 기본 보드로 (검색어는 유지)
  useEffect(() => {
    if (routeId || !boards) return;
    const id = defaultBoard(boards);
    if (id) nav(`/admin/board/${encodeURIComponent(id)}${params.toString() ? `?${params}` : ''}`, { replace: true });
  }, [routeId, boards, nav, params]);
  useEffect(() => { if (routeId) writeJSON(LAST_BOARD, routeId); }, [routeId]);

  // ---------- 보드 보기 (열 + 걸러진 카드) ----------
  const [view, setView] = useState<BoardView | null>(null);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const viewRef = useRef(view);
  viewRef.current = view;
  const req = useRef(0);
  const load = useCallback(async () => {
    if (!routeId) return;
    const n = ++req.current;
    setLoading(true);
    setError('');
    try {
      const v = await workApi.board(routeId, JSON.parse(queryKey) as ReturnType<typeof toQuery>);
      if (n === req.current) setView(v);
    } catch (e) {
      if (n === req.current) setError(errorMessage(e, '보드를 불러오지 못했어요.'));
    } finally {
      if (n === req.current) setLoading(false);
    }
  }, [routeId, queryKey]);
  // 다른 보드로 가면 이전 보드를 지우고 스켈레톤부터
  useEffect(() => { setView(null); }, [routeId]);
  useEffect(() => { void load(); }, [load]);

  const board = view?.board;
  const canWrite = !!board?.access.write;
  const canManage = !!board?.access.manage;

  // 화면 낭독기 알림 — 상태로 두지 않고 직접 써서 보드를 다시 그리지 않는다
  const live = useRef<HTMLParagraphElement>(null);
  const announce = useCallback((msg: string) => {
    const el = live.current;
    if (!el) return;
    el.textContent = '';
    window.setTimeout(() => { el.textContent = msg; }, 30);
  }, []);

  // ---------- 필터 ↔ 주소 ----------
  const paramsRef = useRef(params);
  paramsRef.current = params;
  const filtersRef = useRef(filters);
  filtersRef.current = filters;
  const changeFilters = useCallback((patch: Partial<BoardFilters>) => {
    setParams(filtersToParams({ ...filtersRef.current, ...patch }, paramsRef.current), { replace: true });
  }, [setParams]);
  const clearFilters = useCallback(() => setParams(filtersToParams(EMPTY_FILTERS, paramsRef.current), { replace: true }), [setParams]);

  // ---------- 카드 옮기기 ----------
  const busy = useRef(new Set<string>());
  const moveCard = useCallback(async (cardId: string, columnId: string, index: number) => {
    const v = viewRef.current;
    if (!v || !v.board.access.write) return;
    if (busy.current.has(cardId)) { toast('이 카드를 옮기는 중이에요. 잠시 후 다시 시도해 주세요.', 'info'); return; }
    const col = v.columns.find(c => c.id === columnId);
    const prev = v.cards.find(c => c.id === cardId);
    if (!col || !prev) return;
    const r = applyMove(v.cards, cardId, col, index);
    if (!r) return;
    busy.current.add(cardId);
    const from = prev.column_id;
    // 화면 먼저 — 옮긴 카드만 새 객체라 다른 카드는 다시 그리지 않는다
    setView(cur => (cur ? { ...cur, cards: cur.cards.map(c => (c.id === cardId ? r.card : c)), columns: shiftCount(cur.columns, from, col.id) } : cur));
    announce(`${prev.key} 카드를 ${col.name} 열 ${r.index + 1}번째로 옮기는 중이에요.`);
    try {
      const res = await workApi.moveCard(cardId, { row_version: prev.row_version, column_id: col.id, before_id: r.before_id, after_id: r.after_id });
      const landed = res.card.column_id ?? col.id;
      setView(cur => (cur ? {
        ...cur,
        cards: cur.cards.map(c => (c.id === cardId ? res.card : c)),
        columns: shiftCount(cur.columns, col.id, landed),
      } : cur));
      if (res.redirected) {
        const name = viewRef.current?.columns.find(c => c.id === landed)?.name ?? res.card.column_name ?? '검토';
        toast(`검토자가 있는 카드는 바로 완료할 수 없어 검토 요청으로 ‘${name}’ 열에 놓았어요. 검토자가 승인하면 완료돼요.`, 'info');
        announce(`${prev.key} 카드는 검토자 규칙에 따라 ${name} 열로 옮겨졌어요.`);
      } else {
        announce(`${prev.key} 카드를 ${col.name} 열로 옮겼어요.`);
      }
    } catch (e) {
      // 옮기기 전 카드 그대로 되돌린다
      setView(cur => (cur ? { ...cur, cards: cur.cards.map(c => (c.id === cardId ? prev : c)), columns: shiftCount(cur.columns, col.id, from) } : cur));
      const msg = boardError(e, '카드를 옮기지 못했어요.');
      toast(msg, 'error');
      announce(`옮기지 못했어요. ${msg}`);
      if (needsReload(e)) void load();
    } finally {
      busy.current.delete(cardId);
    }
  }, [announce, load, toast]);

  // 카드·열에 넘기는 동작은 한 번만 만든다 (memo된 카드가 다시 그려지지 않게)
  const drag = useRef<DragInfo | null>(null);
  const moveRef = useRef(moveCard);
  moveRef.current = moveCard;
  const focusCard = (id: string) => requestAnimationFrame(() => {
    const el = document.querySelector<HTMLElement>(cardSelector(id));
    el?.focus({ preventScroll: true });
    el?.scrollIntoView?.({ block: 'nearest', inline: 'nearest' });
  });
  const handlers = useMemo<CardHandlers>(() => ({
    open: id => {
      const next = new URLSearchParams(paramsRef.current);
      next.set('card', id);
      setParams(next, { replace: true });
    },
    keyMove: (id: string, dir: MoveDir) => {
      const v = viewRef.current;
      if (!v) return;
      const t = moveTarget(v.cards, v.columns, id, dir);
      if (!t) { announce(dir === 'left' || dir === 'right' ? '더 옮길 열이 없어요.' : '더 옮길 자리가 없어요.'); return; }
      void moveRef.current(id, t.columnId, t.index);
      focusCard(id);
    },
    moveTo: (id: string, columnId: string) => {
      const v = viewRef.current;
      if (!v) return;
      const n = v.cards.filter(c => c.column_id === columnId && c.id !== id).length;
      void moveRef.current(id, columnId, n);
      focusCard(id);
    },
    dragStart: (id: string, e: DragEvent<HTMLElement>) => {
      const card = viewRef.current?.cards.find(c => c.id === id);
      drag.current = { id, from: card?.column_id ?? '' };
      e.dataTransfer.effectAllowed = 'move';
      e.dataTransfer.setData('text/plain', id);
      e.currentTarget.classList.add('is-dragging');
    },
    dragEnd: (e: DragEvent<HTMLElement>) => {
      e.currentTarget.classList.remove('is-dragging');
      drag.current = null;
    },
  }), [announce, setParams]);
  const onDrop = useCallback((cardId: string, columnId: string, index: number) => { void moveRef.current(cardId, columnId, index); }, []);

  // ---------- 빠른 추가 ----------
  const addCard = useCallback((t: WorkTask) => {
    setView(cur => {
      if (!cur || t.board_id !== cur.board.id || cur.cards.some(c => c.id === t.id)) return cur;
      return { ...cur, cards: [...cur.cards, t], columns: cur.columns.map(c => (c.id === t.column_id ? { ...c, count: c.count + 1 } : c)) };
    });
  }, []);
  const quickAdd = useCallback(async (columnId: string, title: string) => {
    const v = viewRef.current;
    if (!v) return false;
    try {
      const d = await workApi.createTask({ title, type: 'GENERAL', priority: 'NORMAL', board_id: v.board.id, column_id: columnId });
      addCard(d.task);
      announce(`${d.task.key} 카드를 추가했어요.`);
      return true;
    } catch (e) {
      toast(boardError(e, '카드를 만들지 못했어요.'), 'error');
      return false;
    }
  }, [addCard, announce, toast]);

  // ---------- 서랍에서 바뀐 카드 ----------
  const replaceCard = useCallback((t: WorkTask) => {
    setView(cur => {
      if (!cur) return cur;
      const old = cur.cards.find(c => c.id === t.id);
      if (!old || (old.row_version === t.row_version && old.note_count === t.note_count && old.links.length === t.links.length)) return cur;
      return { ...cur, cards: cur.cards.map(c => (c.id === t.id ? t : c)) };
    });
  }, []);
  const removeCard = useCallback((id: string) => {
    setView(cur => {
      const old = cur?.cards.find(c => c.id === id);
      if (!cur || !old) return cur;
      return { ...cur, cards: cur.cards.filter(c => c.id !== id), columns: cur.columns.map(c => (c.id === old.column_id ? { ...c, count: Math.max(0, c.count - 1) } : c)) };
    });
  }, []);
  const closeDrawer = useCallback(() => {
    const next = new URLSearchParams(paramsRef.current);
    next.delete('card');
    setParams(next, { replace: true });
  }, [setParams]);
  const archived = useCallback((id: string) => { removeCard(id); closeDrawer(); }, [removeCard, closeDrawer]);

  // ---------- 열별 카드 (바뀐 열만 새 배열) ----------
  const groups = useRef<Map<string, WorkTask[]>>(new Map());
  const byColumn = useMemo(() => {
    if (!view) return new Map<string, WorkTask[]>();
    groups.current = groupCards(view.cards, view.columns, groups.current);
    return groups.current;
  }, [view]);
  const columnIds = view?.columns.map(c => `${c.id}:${c.name}`).join('|') ?? '';
  const columnOptions = useMemo<ColumnOption[]>(
    () => (view?.columns ?? []).map(c => ({ id: c.id, name: c.name })),
    [columnIds],
  );

  // ---------- 대화상자 ----------
  const [dialog, setDialog] = useState<null | 'card' | 'columns' | 'board' | 'rules'>(null);
  const archiveBoard = async () => {
    if (!board) return;
    if (!(await confirm({ title: `‘${board.name}’ 보드를 보관할까요?`, message: '보드와 자동 규칙이 꺼져요. 카드(업무)는 업무 목록에 그대로 남아요.', confirmLabel: '보드 보관', danger: true }))) return;
    try {
      await workApi.archiveBoard(board.id);
      toast('보드를 보관했어요.', 'success');
      writeJSON(LAST_BOARD, null);
      setBoards(null);
      loadBoards();
      nav('/admin/board', { replace: true });
    } catch (e) {
      toast(boardError(e, '보드를 보관하지 못했어요.'), 'error');
    }
  };
  const boardCreated = (v: BoardView) => {
    loadBoards();
    nav(`/admin/board/${encodeURIComponent(v.board.id)}`);
  };
  const viewChanged = useCallback((v: BoardView) => {
    // 열 관리 응답은 필터 없는 보기라 카드는 현재 필터로 다시 불러온다
    setView(cur => (cur ? { ...cur, board: v.board, columns: v.columns, tags: v.tags } : v));
    void load();
    loadBoards();
  }, [load, loadBoards]);

  // ---------- 보드 고르기 ----------
  const grouped = useMemo(() => KIND_ORDER.map(k => [k, (boards ?? []).filter(b => b.kind === k)] as const).filter(([, list]) => list.length > 0), [boards]);
  const pickBoard = (id: string) => {
    // 열 필터는 보드마다 달라 빼고, 나머지 필터는 그대로 가져간다
    const qs = filtersToParams({ ...filters, column: '' }, new URLSearchParams()).toString();
    nav(`/admin/board/${encodeURIComponent(id)}${qs ? `?${qs}` : ''}`);
  };

  const filtered = activeFilters(filters).length > 0;
  const totalCards = view?.cards.length ?? 0;
  const scrollToColumn = (id: string) => document.getElementById(`kbcol-${id}`)?.scrollIntoView?.({ inline: 'start', block: 'nearest', behavior: window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth' });

  const head = (
    <PageHeader
      eyebrow="업무 · 보드"
      title={board ? board.name : '보드'}
      sub={board ? (
        <span className="adm-kb-sub">
          <Badge tone={board.kind === 'PERSONAL' ? 'violet' : board.kind === 'TEAM' ? 'blue' : 'green'}>{BOARD_KIND[board.kind]} 보드{board.team_role ? ` · ${ROLE_NAME[board.team_role]}` : ''}</Badge>
          {board.description && <span>{board.description}</span>}
        </span>
      ) : '카드를 끌어 옮기거나, 카드에서 Alt+화살표로 옮길 수 있어요.'}
      actions={<>
        {canWrite && <button type="button" className="ui-btn is-primary is-sm" onClick={() => setDialog('card')}><Glyph name="plus" size={14} />새 카드</button>}
        {canManage && <button type="button" className="ui-btn is-soft is-sm" onClick={() => setDialog('columns')}>열 관리</button>}
        {admin && <button type="button" className="ui-btn is-soft is-sm" onClick={() => setDialog('rules')}>자동 규칙</button>}
        <button type="button" className="ui-btn is-soft is-sm" onClick={() => setDialog('board')}>보드 만들기</button>
        {canManage && board?.key !== 'general' && board && <button type="button" className="ui-btn is-danger is-sm" onClick={() => void archiveBoard()}>보관</button>}
      </>}
    />
  );

  return (
    <div className="view-enter adm-kb">
      {head}
      <WorkTabs />
      <p id="kbCardHint" className="sr-only">카드를 열려면 Enter. {canWrite ? 'Alt와 왼쪽·오른쪽 화살표로 옆 열로, Alt와 위·아래 화살표로 순서를 옮겨요.' : '이 보드는 보기만 할 수 있어요.'}</p>
      <p ref={live} className="sr-only" aria-live="polite" aria-atomic="true" />

      <div className="adm-kb-bar">
        <label htmlFor="kbBoard" className="sr-only">보드 고르기</label>
        <select id="kbBoard" className="ui-select is-auto adm-kb-picker-select" value={routeId ?? ''} disabled={!boards} onChange={e => pickBoard(e.target.value)}>
          {!boards && <option value={routeId ?? ''}>보드 불러오는 중…</option>}
          {routeId && boards && !boards.some(b => b.id === routeId) && <option value={routeId}>{board?.name ?? '선택한 보드'}</option>}
          {grouped.map(([k, list]) => (
            <optgroup key={k} label={`${BOARD_KIND[k]} 보드`}>
              {list.map(b => <option key={b.id} value={b.id}>{b.name} · 진행 {b.open_cards ?? 0}</option>)}
            </optgroup>
          ))}
        </select>
        <button type="button" className="ui-btn is-soft is-sm" onClick={() => void load()} disabled={loading}>{loading && view ? '불러오는 중…' : '새로고침'}</button>
      </div>
      {boardsError && <ErrorBox message={boardsError} onRetry={loadBoards} />}

      {boards && boards.length === 0 && !routeId ? (
        <EmptyState icon={<Glyph name="task" size={22} />} title="볼 수 있는 보드가 없어요">
          <button type="button" className="ui-btn is-soft is-sm" style={{ marginTop: 14 }} onClick={() => setDialog('board')}>보드 만들기</button>
        </EmptyState>
      ) : (
        <>
          <FilterBar filters={filters} onChange={changeFilters} onClear={clearFilters} people={people} columns={view?.columns ?? []} tags={view?.tags ?? []} boardKind={board?.kind} />
          {error && <ErrorBox message={error} onRetry={() => void load()} />}
          {view && !canWrite && (
            <div className="ui-callout adm-kb-note"><Glyph name="lock" size={13} className="aq-inline-glyph" /> 이 보드는 보기만 할 수 있어요. {board?.archived ? '보관된 보드예요.' : board?.kind === 'TEAM' ? `${board.team_role ? ROLE_NAME[board.team_role] : '해당'} 팀과 관리자만 카드를 옮기고 추가할 수 있어요.` : '카드 담당·검토자는 카드 상세에서 내용을 바꿀 수 있어요.'}</div>
          )}
          {view?.truncated && <div className="ui-callout is-warn">카드가 많아 1,500장까지만 보여요. 필터로 범위를 좁혀 주세요.</div>}
          {view && filtered && totalCards === 0 && (
            <div className="ui-callout is-row"><span>조건에 맞는 카드가 없어요.</span><button type="button" className="ui-btn is-soft is-sm" onClick={clearFilters}>필터 지우기</button></div>
          )}
          {view && !filtered && totalCards === 0 && (
            <p className="adm-kb-empty-board">아직 카드가 없어요. {canWrite ? '열 아래 ‘카드 추가’나 위의 ‘새 카드’로 시작해 보세요.' : ''}</p>
          )}

          {!view ? (error ? null : <SkeletonList rows={6} />) : (
            <>
              <nav className="adm-kb-jump" aria-label="열로 이동">
                {view.columns.map(c => (
                  <button key={c.id} type="button" onClick={() => scrollToColumn(c.id)}>{c.name}<span className="ui-count">{byColumn.get(c.id)?.length ?? 0}</span></button>
                ))}
              </nav>
              <div className={`adm-kb-board${loading ? ' is-loading' : ''}`} aria-busy={loading} aria-label={`${view.board.name} 보드 열`} role="region" tabIndex={-1}>
                {view.columns.map(col => (
                  <BoardColumnView
                    key={col.id} column={col} cards={byColumn.get(col.id) ?? []} people={people} canWrite={canWrite}
                    columnOptions={columnOptions} h={handlers} drag={drag} onDrop={onDrop} onQuickAdd={quickAdd} filtered={filtered}
                  />
                ))}
                {view.columns.length === 0 && <p className="adm-kb-empty-board">열이 없어요. {canManage ? '‘열 관리’에서 열을 추가해 주세요.' : ''}</p>}
              </div>
            </>
          )}
        </>
      )}

      {openCard && (
        <CardDrawer cardId={openCard} boardWritable={canWrite} onClose={closeDrawer} onChanged={replaceCard} onArchived={archived} />
      )}
      {dialog === 'card' && view && (
        <Modal title="새 카드" onClose={() => setDialog(null)} dismissible={false} modalClass="adm-w-modal">
          <NewCardForm view={view} onCreated={addCard} />
        </Modal>
      )}
      {dialog === 'columns' && view && (
        <Modal title={`열 관리 · ${view.board.name}`} onClose={() => setDialog(null)} modalClass="adm-w-modal">
          <ColumnManager view={view} onView={viewChanged} />
        </Modal>
      )}
      {dialog === 'board' && (
        <Modal title="보드 만들기" onClose={() => setDialog(null)} dismissible={false}>
          <BoardCreateForm onCreated={boardCreated} />
        </Modal>
      )}
      {dialog === 'rules' && (
        <Modal title="자동 규칙" onClose={() => setDialog(null)} modalClass="adm-w-modal">
          <RulesPanel boards={boards ?? []} onRan={() => { void load(); loadBoards(); }} />
        </Modal>
      )}
    </div>
  );
}

