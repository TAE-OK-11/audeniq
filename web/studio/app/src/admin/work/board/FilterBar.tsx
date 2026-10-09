// 보드 필터 줄 — 값은 주소 검색어(필터 ↔ URL)에 두어 새로고침·공유해도 같은 화면이 나온다.
// 검색어는 입력이 멈추고 250ms 뒤에 주소로 보낸다(그 사이 키 입력마다 보드를 다시 부르지 않게).
import { memo, useEffect, useRef, useState } from 'react';
import { useStaff } from '../../ui';
import { BOARD_KIND, CARD_SOURCE, ENTITY_KIND, LINK_KINDS, PRIORITIES, PRIORITY_CODE, PRIORITY_NAME, WORK_TYPE, WORK_TYPES } from '../labels';
import { DONE_DAYS, DUE_OPTIONS, SOURCES, activeFilters, type BoardFilters, type FilterKey } from './logic';
import type { People } from '../ui';
import type { BoardColumn } from '../types';

export const DUE_LABEL: Record<string, string> = { overdue: '기한 지남', today: '오늘까지', week: '7일 안', none: '마감 없음' };
export const SOURCE_LABEL = (s: string) => (s === 'manual' ? '직접 만든 카드' : s === 'auto' ? '자동 카드 전체' : `자동 · ${CARD_SOURCE[s] ?? s}`);
export const SEARCH_DEBOUNCE = 250;

const FILTER_NAME: Record<FilterKey, string> = {
  assignee: '담당', priority: '우선순위', column: '열', tag: '태그', type: '유형', due: '마감', creator: '만든 사람', source: '출처',
  entity_kind: '연결', q: '검색', done_days: '완료 카드',
};

export const FilterBar = memo(function FilterBar({ filters, onChange, onClear, people, columns, tags, boardKind }: {
  filters: BoardFilters;
  onChange: (patch: Partial<BoardFilters>) => void;
  onClear: () => void;
  people: People;
  columns: BoardColumn[];
  tags: string[];
  boardKind?: string;
}) {
  const { me } = useStaff();
  const [q, setQ] = useState(filters.q);
  const typing = useRef(false);
  // 주소가 바뀌면(뒤로 가기·지우기) 입력칸도 맞춘다 — 입력 중에는 덮어쓰지 않는다
  useEffect(() => { if (!typing.current) setQ(filters.q); }, [filters.q]);
  useEffect(() => {
    if (!typing.current) return;
    const t = window.setTimeout(() => { typing.current = false; if (q !== filters.q) onChange({ q }); }, SEARCH_DEBOUNCE);
    return () => window.clearTimeout(t);
  }, [q, filters.q, onChange]);

  const person = (id: string) => (id === 'me' || id === me.user_id ? '나' : id === 'none' ? '미배정' : people.get(id)?.email ?? '선택한 사람');
  const valueText = (k: FilterKey): string => {
    const v = filters[k];
    switch (k) {
      case 'assignee': case 'creator': return person(String(v));
      case 'priority': return filters.priority ? `${PRIORITY_CODE[filters.priority]} ${PRIORITY_NAME[filters.priority]}` : '';
      case 'column': return columns.find(c => c.id === v)?.name ?? '선택한 열';
      case 'type': return filters.type ? WORK_TYPE[filters.type] : '';
      case 'due': return DUE_LABEL[String(v)] ?? '';
      case 'source': return SOURCE_LABEL(String(v));
      case 'entity_kind': return filters.entity_kind ? ENTITY_KIND[filters.entity_kind].label : '';
      case 'tag': return `#${v}`;
      default: return String(v ?? '');
    }
  };
  const active = activeFilters(filters);
  const others = people.list.filter(p => p.user_id !== me.user_id);

  return (
    <div className="adm-kb-filters" role="search" aria-label={`${boardKind ? `${BOARD_KIND[boardKind] ?? ''} ` : ''}보드 카드 거르기`}>
      <div className="adm-kb-filter-row">
        <label htmlFor="kbSearch" className="sr-only">카드 찾기</label>
        <input
          id="kbSearch" type="search" className="ui-input adm-kb-search" placeholder="제목·내용·번호·연결 이름으로 찾기" value={q} maxLength={80}
          onChange={e => { typing.current = true; setQ(e.target.value); }}
        />
        <select className="ui-select is-auto" aria-label="담당자" value={filters.assignee} onChange={e => onChange({ assignee: e.target.value })}>
          <option value="">모든 담당자</option>
          <option value="me">나</option>
          <option value="none">미배정</option>
          {others.map(p => <option key={p.user_id} value={p.user_id}>{p.email}</option>)}
        </select>
        <select className="ui-select is-auto" aria-label="우선순위" value={filters.priority} onChange={e => onChange({ priority: e.target.value as BoardFilters['priority'] })}>
          <option value="">모든 우선순위</option>
          {PRIORITIES.map(p => <option key={p} value={p}>{PRIORITY_CODE[p]} · {PRIORITY_NAME[p]}</option>)}
        </select>
        <select className="ui-select is-auto" aria-label="마감" value={filters.due} onChange={e => onChange({ due: e.target.value as BoardFilters['due'] })}>
          <option value="">모든 마감</option>
          {DUE_OPTIONS.map(d => <option key={d} value={d}>{DUE_LABEL[d]}</option>)}
        </select>
        <select className="ui-select is-auto" aria-label="출처" value={filters.source} onChange={e => onChange({ source: e.target.value as BoardFilters['source'] })}>
          <option value="">직접·자동 모두</option>
          <option value="manual">{SOURCE_LABEL('manual')}</option>
          <option value="auto">{SOURCE_LABEL('auto')}</option>
          {SOURCES.map(s => <option key={s} value={s}>{SOURCE_LABEL(s)}</option>)}
        </select>
      </div>
      <details className="adm-kb-more" open={active.some(k => ['column', 'tag', 'type', 'creator', 'entity_kind'].includes(k)) || undefined}>
        <summary>필터 더 보기</summary>
        <div className="adm-kb-filter-row">
          <select className="ui-select is-auto" aria-label="열" value={filters.column} onChange={e => onChange({ column: e.target.value })}>
            <option value="">모든 열</option>
            {columns.map(c => <option key={c.id} value={c.id}>{c.name}</option>)}
          </select>
          <select className="ui-select is-auto" aria-label="태그" value={filters.tag} onChange={e => onChange({ tag: e.target.value })}>
            <option value="">모든 태그</option>
            {filters.tag && !tags.includes(filters.tag) && <option value={filters.tag}>#{filters.tag}</option>}
            {tags.map(t => <option key={t} value={t}>#{t}</option>)}
          </select>
          <select className="ui-select is-auto" aria-label="업무 유형" value={filters.type} onChange={e => onChange({ type: e.target.value as BoardFilters['type'] })}>
            <option value="">모든 유형</option>
            {WORK_TYPES.map(t => <option key={t} value={t}>{WORK_TYPE[t]}</option>)}
          </select>
          <select className="ui-select is-auto" aria-label="만든 사람" value={filters.creator} onChange={e => onChange({ creator: e.target.value })}>
            <option value="">만든 사람 전체</option>
            {people.list.map(p => <option key={p.user_id} value={p.user_id}>{p.user_id === me.user_id ? `나 (${p.email})` : p.email}</option>)}
          </select>
          <select className="ui-select is-auto" aria-label="연결된 항목 종류" value={filters.entity_kind} onChange={e => onChange({ entity_kind: e.target.value as BoardFilters['entity_kind'] })}>
            <option value="">연결 전체</option>
            {LINK_KINDS.map(k => <option key={k} value={k}>{ENTITY_KIND[k].label} 연결</option>)}
          </select>
          <select className="ui-select is-auto" aria-label="완료 카드 보기 기간" value={filters.done_days ?? ''} onChange={e => onChange({ done_days: e.target.value ? Number(e.target.value) : null })}>
            <option value="">완료 카드 최근 30일</option>
            {DONE_DAYS.filter(d => d !== 30).map(d => <option key={d} value={d}>완료 카드 최근 {d}일</option>)}
          </select>
        </div>
      </details>
      {active.length > 0 && (
        <div className="adm-kb-chips" aria-label="켜진 필터">
          {active.map(k => (
            <button key={k} type="button" className="adm-kb-chip" onClick={() => { if (k === 'q') { typing.current = false; setQ(''); } onChange({ [k]: k === 'done_days' ? null : '' }); }}>
              <span>{FILTER_NAME[k]}: {valueText(k)}</span>
              <span aria-hidden="true">×</span><span className="sr-only"> 필터 끄기</span>
            </button>
          ))}
          <button type="button" className="ui-btn is-text is-muted" onClick={() => { typing.current = false; setQ(''); onClear(); }}>모두 지우기</button>
        </div>
      )}
    </div>
  );
});
