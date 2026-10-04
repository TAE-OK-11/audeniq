// 칸반 열 — 머리(이름·카드 수·WIP·상태), 카드 목록, 끌어 놓을 자리 표시선, 빠른 추가.
// 끌고 있는 카드는 보드의 ref에만 두고, 놓을 자리(표시선)는 이 열의 상태로만 바꿔 다른 열·카드는 다시 그리지 않는다.
import { memo, useEffect, useRef, useState, type DragEvent, type RefObject } from 'react';
import { Glyph } from '../../../components/Glyph';
import { TASK_STATUS } from '../labels';
import { BoardCard, type CardHandlers, type ColumnOption } from './Card';
import { insertIndex, wipState } from './logic';
import type { People } from '../ui';
import type { BoardColumn, WorkTask } from '../types';

export interface DragInfo { id: string; from: string }

export const BoardColumnView = memo(function BoardColumnView({
  column, cards, people, canWrite, columnOptions, h, drag, onDrop, onQuickAdd, filtered,
}: {
  column: BoardColumn;
  cards: WorkTask[];
  people: People;
  canWrite: boolean;
  columnOptions: ColumnOption[];
  h: CardHandlers;
  drag: RefObject<DragInfo | null>;
  onDrop: (cardId: string, columnId: string, index: number) => void;
  onQuickAdd: (columnId: string, title: string) => Promise<boolean>;
  filtered: boolean;
}) {
  const [over, setOver] = useState<number | null>(null);
  const overRef = useRef<number | null>(null);
  const listRef = useRef<HTMLOListElement>(null);
  const [adding, setAdding] = useState(false);
  const [title, setTitle] = useState('');
  const [busy, setBusy] = useState(false);
  const addBtn = useRef<HTMLButtonElement>(null);

  const setIndex = (i: number | null) => {
    if (overRef.current === i) return;
    overRef.current = i;
    setOver(i);
  };
  // 보드 밖에 놓거나 취소하면 표시선을 지운다
  useEffect(() => {
    if (over == null) return;
    const clear = () => setIndex(null);
    document.addEventListener('dragend', clear);
    document.addEventListener('drop', clear);
    return () => { document.removeEventListener('dragend', clear); document.removeEventListener('drop', clear); };
  }, [over]);

  const indexAt = (y: number) => {
    const id = drag.current?.id;
    const mids: number[] = [];
    for (const el of listRef.current?.querySelectorAll<HTMLElement>('[data-card-id]') ?? []) {
      if (el.dataset.cardId === id) continue;
      const r = el.getBoundingClientRect();
      mids.push(r.top + r.height / 2);
    }
    return insertIndex(mids, y);
  };
  const dragOver = (e: DragEvent<HTMLElement>) => {
    if (!drag.current || !canWrite) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
    setIndex(indexAt(e.clientY));
  };
  const dragLeave = (e: DragEvent<HTMLElement>) => {
    if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setIndex(null);
  };
  const drop = (e: DragEvent<HTMLElement>) => {
    const d = drag.current;
    if (!d || !canWrite) return;
    e.preventDefault();
    const index = overRef.current ?? indexAt(e.clientY);
    setIndex(null);
    onDrop(d.id, column.id, index);
  };

  const submit = async () => {
    const name = title.trim();
    if (!name || busy) return;
    setBusy(true);
    const ok = await onQuickAdd(column.id, name);
    setBusy(false);
    if (ok) setTitle('');
  };
  const cancel = () => {
    setAdding(false);
    setTitle('');
    requestAnimationFrame(() => addBtn.current?.focus());
  };

  const wip = wipState(column);
  const [statusLabel] = TASK_STATUS[column.status];
  const dragging = drag.current?.id;
  // 표시선은 끌고 있는 카드를 뺀 목록 기준의 자리에 그린다
  let k = 0;
  const line = <li key="__drop" className="adm-kb-drop" aria-hidden="true" />;
  const canQuickAdd = canWrite && column.status !== 'IN_REVIEW';

  return (
    <section
      id={`kbcol-${column.id}`}
      className={`adm-kb-col${over != null ? ' is-over' : ''}${wip === 'over' ? ' is-over-wip' : ''}`}
      aria-labelledby={`kbcol-h-${column.id}`}
      onDragOver={dragOver} onDragLeave={dragLeave} onDrop={drop}
    >
      <header className="adm-kb-col-head">
        <h3 id={`kbcol-h-${column.id}`}>
          <span className={`adm-kb-dot is-${column.status.toLowerCase()}`} aria-hidden="true" />
          <span className="adm-kb-col-name">{column.name}</span>
          <b className="adm-count">{cards.length}<span className="sr-only">개</span></b>
        </h3>
        <span className="adm-kb-col-sub">
          <span>{statusLabel}</span>
          {column.wip_limit != null && (
            <span className={`adm-kb-wip${wip === 'over' ? ' is-over' : wip === 'full' ? ' is-full' : ''}`}>
              {wip === 'over' && <Glyph name="warning" size={11} />}
              WIP {column.count}/{column.wip_limit}{wip === 'over' ? ' · 한도 초과' : wip === 'full' ? ' · 한도 도달' : ''}
            </span>
          )}
          {filtered && cards.length !== column.count && <span className="muted">전체 {column.count}</span>}
        </span>
      </header>
      <ol ref={listRef} className="adm-kb-cards" aria-label={`${column.name} 카드`}>
        {cards.map(c => {
          const before = c.id !== dragging && k === over ? line : null;
          if (c.id !== dragging) k++;
          return [before, <li key={c.id}><BoardCard card={c} people={people} canWrite={canWrite} columns={columnOptions} h={h} /></li>];
        })}
        {over != null && over >= k && line}
        {cards.length === 0 && over == null && <li className="adm-kb-empty">{filtered ? '조건에 맞는 카드가 없어요' : canWrite ? '카드를 끌어 놓거나 아래에서 추가해요' : '카드가 없어요'}</li>}
      </ol>
      {canQuickAdd && (adding ? (
        <form className="adm-kb-add" onSubmit={e => { e.preventDefault(); void submit(); }}>
          <label htmlFor={`kbadd-${column.id}`} className="sr-only">{column.name}에 추가할 카드 제목</label>
          <input
            id={`kbadd-${column.id}`} className="adm-input" autoFocus maxLength={200} value={title} placeholder="카드 제목 · Enter로 추가"
            onChange={e => setTitle(e.target.value)} onKeyDown={e => { if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); cancel(); } }}
          />
          <span className="adm-w-btns">
            <button type="submit" className="adm-btn primary small" disabled={!title.trim() || busy}>{busy ? '추가 중…' : '추가'}</button>
            <button type="button" className="adm-btn soft small" onClick={cancel}>닫기</button>
          </span>
        </form>
      ) : (
        <button ref={addBtn} type="button" className="adm-kb-addbtn" onClick={() => setAdding(true)}>
          <Glyph name="plus" size={13} />카드 추가<span className="sr-only"> ({column.name})</span>
        </button>
      ))}
    </section>
  );
});
