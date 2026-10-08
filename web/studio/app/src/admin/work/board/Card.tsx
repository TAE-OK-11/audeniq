// 칸반 카드 — memo로 감싸 끌기·필터 입력 중에 다른 카드가 다시 그려지지 않게 한다.
// 카드 전체가 눌리는 영역(제목 버튼을 늘린 것)이고, 키보드로는 Alt+화살표 또는 '옮기기' 선택으로 옮긴다.
import { memo, type DragEvent, type KeyboardEvent } from 'react';
import { Glyph } from '../../../components/Glyph';
import { CARD_SOURCE, PRIORITY, PRIORITY_CODE, PRIORITY_NAME, WORK_TYPE } from '../labels';
import { DueChip, EntityChips, Person, type People } from '../ui';
import type { MoveDir } from './logic';
import type { Priority, WorkTask } from '../types';

/** 카드가 부르는 동작 — 보드가 한 번 만들어 같은 객체를 계속 넘긴다 */
export interface CardHandlers {
  open: (id: string) => void;
  keyMove: (id: string, dir: MoveDir) => void;
  moveTo: (id: string, columnId: string) => void;
  dragStart: (id: string, e: DragEvent<HTMLElement>) => void;
  dragEnd: (e: DragEvent<HTMLElement>) => void;
}
export interface ColumnOption { id: string; name: string }

const KEY_DIR: Record<string, MoveDir> = { ArrowLeft: 'left', ArrowRight: 'right', ArrowUp: 'up', ArrowDown: 'down' };

/** 우선순위 — 아이콘 모양 + P0 글자 (색만으로 구분하지 않는다) */
export function PriorityBadge({ value }: { value: Priority }) {
  const path = value === 'URGENT' ? 'M4 9l4-4 4 4M4 13l4-4 4 4' : value === 'HIGH' ? 'M4 11l4-4 4 4' : value === 'NORMAL' ? 'M4 8h8' : 'M4 6l4 4 4-4';
  return (
    <span className={`adm-kb-pri is-${value.toLowerCase()}`} title={`${PRIORITY_NAME[value]} · ${PRIORITY[value][0]}`}>
      <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d={path} /></svg>
      {PRIORITY_CODE[value]}<span className="sr-only"> {PRIORITY_NAME[value]} 우선순위</span>
    </span>
  );
}

export const sourceLabel = (s: WorkTask['source']) => (s && s !== 'manual' ? `자동 · ${CARD_SOURCE[s] ?? s}` : null);

export const BoardCard = memo(function BoardCard({ card, people, canWrite, columns, h }: {
  card: WorkTask; people: People; canWrite: boolean; columns: ColumnOption[]; h: CardHandlers;
}) {
  const closed = card.status === 'DONE' || card.status === 'CANCELLED';
  const late = !closed && (card.overdue || card.sla_state === 'BREACHED');
  const auto = sourceLabel(card.source);
  const onKey = (e: KeyboardEvent<HTMLButtonElement>) => {
    const dir = KEY_DIR[e.key];
    if (!dir || !e.altKey || !canWrite) return;
    e.preventDefault();
    h.keyMove(card.id, dir);
  };
  return (
    <article
      className={`adm-kb-card${late ? ' is-late' : ''}${closed ? ' is-closed' : ''}`}
      data-card-id={card.id}
      draggable={canWrite}
      onDragStart={canWrite ? e => h.dragStart(card.id, e) : undefined}
      onDragEnd={canWrite ? h.dragEnd : undefined}
    >
      <span className="adm-kb-card-top">
        <span className="ui-tag is-mono">{card.key}</span>
        <PriorityBadge value={card.priority} />
        {auto && <span className="adm-kb-source"><Glyph name="bell" size={11} />{auto}</span>}
        {late && <span className="adm-kb-late"><Glyph name="clock" size={11} />기한 지남</span>}
      </span>
      <button
        type="button" className="adm-kb-card-open" data-card-focus={card.id}
        aria-describedby="kbCardHint" aria-keyshortcuts={canWrite ? 'Alt+ArrowLeft Alt+ArrowRight Alt+ArrowUp Alt+ArrowDown' : undefined}
        onClick={() => h.open(card.id)} onKeyDown={onKey}
      >
        {card.title}
      </button>
      <span className="adm-kb-meta">
        <span>{WORK_TYPE[card.type]}</span>
        {card.tags.slice(0, 3).map(t => <span key={t} className="adm-kb-tag">#{t}</span>)}
        {card.tags.length > 3 && <span>+{card.tags.length - 3}</span>}
      </span>
      {card.links.length > 0 && (
        <span className="adm-kb-links">
          <EntityChips items={card.links.slice(0, 2)} plain />
          {card.links.length > 2 && <small>+{card.links.length - 2}</small>}
        </span>
      )}
      <span className="adm-kb-card-foot">
        <Person id={card.assignee_id} people={people} compact />
        <span className="adm-kb-counts">
          {card.subtasks.total > 0 && <span title="하위 업무"><Glyph name="task" size={11} />{card.subtasks.done}/{card.subtasks.total}<span className="sr-only"> 하위 업무 완료</span></span>}
          {card.note_count > 0 && <span title="메모"><Glyph name="inquiry" size={11} />{card.note_count}<span className="sr-only"> 메모</span></span>}
          <DueChip due={card.deadline ?? card.due_at} closed={closed} />
        </span>
      </span>
      {canWrite && (
        <label className="adm-kb-move">
          <span className="sr-only">{card.key} 다른 열로 옮기기</span>
          <Glyph name="arrow-right" size={12} />
          <select className="ui-select is-auto" value={card.column_id ?? ''} onChange={e => h.moveTo(card.id, e.target.value)}>
            {columns.map(c => <option key={c.id} value={c.id}>{c.id === card.column_id ? `${c.name} (지금)` : `${c.name}(으)로 옮기기`}</option>)}
          </select>
        </label>
      )}
    </article>
  );
});
