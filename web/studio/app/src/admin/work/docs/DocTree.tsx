// 문서 트리 — 펼칠 때 그 단계만 불러온다(lazy). 볼 수 없는 문서는 서버가 빼고 준다.
// 키보드: ↑↓ 이동, → 펼치기/첫 하위로, ← 접기/상위로, Enter 열기 (WAI-ARIA tree).
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Link } from '../../../lib/router';
import { Glyph } from '../../../components/Glyph';
import { errorMessage } from '../../../api/errors';
import { workApi } from '../api';
import { DOC_STATUS } from '../labels';
import type { DocSummary } from '../types';

interface Props {
  currentId: string | null;
  /** 바뀌면 다시 불러온다 (만들기·이동·보관 뒤) */
  reloadKey: number;
  /** 열려 있는 문서의 상위들 — 처음 열 때 펼쳐 둔다 */
  ancestors: string[];
  onOpen: (id: string) => void;
  onAddChild: (parentId: string) => void;
  archived?: boolean;
}

type Children = Record<string, DocSummary[] | 'loading' | { error: string }>;
const ROOT = '__root__';

const Row = memo(function Row({ d, depth, open, active, focused, kids, onToggle, onOpen, onAddChild, onFocus }: {
  d: DocSummary; depth: number; open: boolean; active: boolean; focused: boolean; kids: boolean;
  onToggle: (id: string) => void; onOpen: (id: string) => void; onAddChild: (id: string) => void; onFocus: (id: string) => void;
}) {
  return (
    <div
      role="treeitem" aria-level={depth + 1} aria-expanded={kids ? open : undefined} aria-selected={active}
      tabIndex={focused ? 0 : -1} data-id={d.id}
      className={`adm-doc-tree-row${active ? ' is-active' : ''}${d.archived_at ? ' is-archived' : ''}`}
      style={{ paddingLeft: 6 + depth * 14 }}
      onFocus={() => onFocus(d.id)}
      onClick={() => onOpen(d.id)}
    >
      <button type="button" tabIndex={-1} className="adm-doc-tree-caret" aria-hidden="true" disabled={!kids}
        onClick={e => { e.stopPropagation(); onToggle(d.id); }}>{kids ? (open ? '▾' : '▸') : ''}</button>
      <span className="adm-doc-tree-icon" aria-hidden="true">{d.icon || '📄'}</span>
      <span className="adm-doc-tree-title">{d.title || '제목 없음'}</span>
      {d.staff_access == null && <span className="adm-doc-tree-lock" title="제한된 문서"><Glyph name="lock" size={11} /><span className="sr-only">제한된 문서</span></span>}
      {d.status !== 'DRAFT' && <span className={`adm-doc-tree-status is-${d.status.toLowerCase()}`}>{DOC_STATUS[d.status][0]}</span>}
      {!d.archived_at && d.level !== 'VIEWER' && (
        <button type="button" tabIndex={-1} className="adm-doc-tree-add" aria-label={`${d.title} 아래에 문서 만들기`}
          onClick={e => { e.stopPropagation(); onAddChild(d.id); }}><Glyph name="plus" size={12} /></button>
      )}
    </div>
  );
});

export function DocTree({ currentId, reloadKey, ancestors, onOpen, onAddChild, archived = false }: Props) {
  const [kids, setKids] = useState<Children>({});
  const [open, setOpen] = useState<Set<string>>(() => new Set(ancestors));
  const [focus, setFocus] = useState<string | null>(currentId);
  const box = useRef<HTMLDivElement>(null);

  const load = useCallback(async (parent: string | null) => {
    const key = parent ?? ROOT;
    setKids(k => ({ ...k, [key]: 'loading' }));
    try {
      const r = await workApi.docTree(parent, archived);
      setKids(k => ({ ...k, [key]: r.items }));
    } catch (e) {
      setKids(k => ({ ...k, [key]: { error: errorMessage(e, '문서 목록을 불러오지 못했어요.') } }));
    }
  }, [archived]);

  // 처음·새로고침: 맨 위와 펼친 단계만 다시
  useEffect(() => {
    void load(null);
    for (const id of open) void load(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [reloadKey, load]);

  useEffect(() => {
    const missing = ancestors.filter(a => !open.has(a));
    if (missing.length) {
      setOpen(o => new Set([...o, ...missing]));
      for (const a of missing) void load(a);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ancestors.join(',')]);

  const toggle = useCallback((id: string) => {
    setOpen(o => {
      const n = new Set(o);
      if (n.has(id)) n.delete(id);
      else { n.add(id); void load(id); }
      return n;
    });
  }, [load]);

  // 보이는 줄 (키보드 이동용)
  const rows = useMemo(() => {
    const out: { d: DocSummary; depth: number; parent: string | null }[] = [];
    const walk = (parent: string | null, depth: number) => {
      const list = kids[parent ?? ROOT];
      if (!Array.isArray(list)) return;
      for (const d of list) {
        out.push({ d, depth, parent });
        if (open.has(d.id)) walk(d.id, depth + 1);
      }
    };
    walk(null, 0);
    return out;
  }, [kids, open]);

  const focusRow = (id: string) => {
    setFocus(id);
    requestAnimationFrame(() => box.current?.querySelector<HTMLElement>(`[data-id="${CSS.escape(id)}"]`)?.focus());
  };

  const onKey = (e: React.KeyboardEvent) => {
    const i = rows.findIndex(r => r.d.id === focus);
    if (i < 0) return;
    const r = rows[i];
    const hasKids = r.d.has_children ?? false;
    if (e.key === 'ArrowDown' && i < rows.length - 1) { e.preventDefault(); focusRow(rows[i + 1].d.id); }
    else if (e.key === 'ArrowUp' && i > 0) { e.preventDefault(); focusRow(rows[i - 1].d.id); }
    else if (e.key === 'ArrowRight' && hasKids) {
      e.preventDefault();
      if (!open.has(r.d.id)) toggle(r.d.id);
      else if (rows[i + 1]?.parent === r.d.id) focusRow(rows[i + 1].d.id);
    } else if (e.key === 'ArrowLeft') {
      e.preventDefault();
      if (open.has(r.d.id)) toggle(r.d.id);
      else if (r.parent) focusRow(r.parent);
    } else if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onOpen(r.d.id); }
    else if (e.key === 'Home') { e.preventDefault(); if (rows[0]) focusRow(rows[0].d.id); }
    else if (e.key === 'End') { e.preventDefault(); const last = rows.at(-1); if (last) focusRow(last.d.id); }
  };

  const root = kids[ROOT];
  const focusId = focus && rows.some(r => r.d.id === focus) ? focus : rows[0]?.d.id ?? null;
  if (root === undefined || root === 'loading') return <div className="ui-skel-rows" aria-label="문서 목록 불러오는 중"><span /><span /><span /></div>;
  if (!Array.isArray(root)) return <div className="ui-callout is-danger">{root.error} <button type="button" className="ui-btn is-text is-muted" onClick={() => void load(null)}>다시 시도</button></div>;
  if (root.length === 0) return <p className="adm-doc-tree-empty">{archived ? '보관한 문서가 없어요.' : '아직 문서가 없어요.'}</p>;
  return (
    <div ref={box} role="tree" aria-label={archived ? '보관한 문서' : '문서'} className="adm-doc-tree" onKeyDown={onKey}>
      {rows.map(({ d, depth }) => {
        const k = kids[d.id];
        return (
          <div key={d.id} role="none">
            <Row d={d} depth={depth} open={open.has(d.id)} active={d.id === currentId} focused={d.id === focusId}
              kids={d.has_children ?? false} onToggle={toggle} onOpen={onOpen} onAddChild={onAddChild} onFocus={setFocus} />
            {open.has(d.id) && k === 'loading' && <div className="adm-doc-tree-loading" style={{ paddingLeft: 26 + depth * 14 }}>불러오는 중…</div>}
            {open.has(d.id) && k && !Array.isArray(k) && k !== 'loading' && <div className="adm-doc-tree-loading is-error" style={{ paddingLeft: 26 + depth * 14 }}>{k.error}</div>}
          </div>
        );
      })}
    </div>
  );
}

/** 검색 결과 — 제목·본문(전문 검색), 미리보기는 서버가 준 글자만 */
export function DocSearchResults({ q, onOpen }: { q: string; onOpen: (id: string) => void }) {
  const [state, setState] = useState<{ items: DocSummary[] } | { error: string } | null>(null);
  useEffect(() => {
    let alive = true;
    setState(null);
    const t = window.setTimeout(() => {
      workApi.searchDocs({ q, limit: 30 })
        .then(r => { if (alive) setState({ items: r.items }); })
        .catch(e => { if (alive) setState({ error: errorMessage(e, '찾지 못했어요.') }); });
    }, 250);
    return () => { alive = false; window.clearTimeout(t); };
  }, [q]);
  if (!state) return <div className="ui-skel-rows"><span /><span /></div>;
  if ('error' in state) return <div className="ui-callout is-danger">{state.error}</div>;
  if (state.items.length === 0) return <p className="adm-doc-tree-empty">‘{q}’에 맞는 문서가 없어요.</p>;
  return (
    <ul className="adm-doc-results" aria-label="검색 결과">
      {state.items.map(d => (
        <li key={d.id}>
          <Link to={`/admin/docs/${d.id}`} onClick={() => onOpen(d.id)}>
            <b>{d.icon || '📄'} {d.title || '제목 없음'}</b>
            {d.snippet && <small>{d.snippet}</small>}
          </Link>
        </li>
      ))}
    </ul>
  );
}
