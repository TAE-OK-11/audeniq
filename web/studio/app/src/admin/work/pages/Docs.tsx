// AUDENIQ Docs — 직원 내부 문서. 왼쪽 트리(검색·보관함), 가운데 편집기, 오른쪽 댓글·정보·기록.
// 저장은 자동(Autosave: 잠깐 쉬면 묶어서, 실패하면 다시 시도, 충돌은 덮어쓰지 않음)이고 버전 충돌은
// 서버가 409로 알려 준다. 권한(보기·편집·관리·소유)은 서버가 정하고 화면은 그 값을 따른다.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { Editor } from '@tiptap/react';
import { useNavigate, useParams, useSearchParams } from '../../../lib/router';
import { useToast } from '../../../components/Toast';
import { useConfirm } from '../../../components/Confirm';
import { ApiError, errorMessage } from '../../../api/errors';
import { Glyph } from '../../../components/Glyph';
import { Badge, EmptyState, ErrorBox, SkeletonList, useStaff } from '../../ui';
import { workApi } from '../api';
import { DOC_STATUS } from '../labels';
import { usePeople, type People } from '../ui';
import type { DocDetail, DocNode, DocSaveInput, DocTemplate } from '../types';
import { Autosave, type AutosaveSnapshot } from '../docs/autosave';
import { EmbedStore } from '../docs/embeds';
import { normalizeDoc } from '../docs/model';
import { DocsContext, type DocsCtx } from '../docs/nodeviews';
import { DocEditor, type CommentAnchor } from '../docs/DocEditor';
import { DocSearchResults, DocTree } from '../docs/DocTree';
import { CommentsPanel, InfoPanel, VersionsPanel } from '../docs/DocPanels';
import { MoveDialog, NewDocDialog, ReasonDialog, ShareDialog, TemplateDialog } from '../docs/DocDialogs';
import '../../../styles/docs.css';

type Panel = 'comments' | 'info' | 'versions' | null;
const LEVEL_RANK = { NONE: 0, VIEWER: 1, EDITOR: 2, MANAGER: 3, OWNER: 4 } as const;

function SaveStatus({ s, onRetry }: { s: AutosaveSnapshot; onRetry: () => void }) {
  const at = s.savedAt ? new Date(s.savedAt).toLocaleTimeString('ko-KR', { hour: '2-digit', minute: '2-digit' }) : '';
  const text = s.state === 'saving' ? '저장 중…' : s.state === 'dirty' ? '변경 사항 저장 대기' : s.state === 'saved' ? `저장됨 ${at}`
    : s.state === 'error' ? '저장하지 못했어요' : s.state === 'conflict' ? '다른 곳에서 고쳐졌어요' : '';
  return (
    <span className={`adm-doc-save is-${s.state}`} role="status" aria-live="polite">
      {text}
      {s.state === 'error' && <button type="button" className="ui-btn is-text is-muted" onClick={onRetry}>다시 시도</button>}
    </span>
  );
}

function MoreMenu({ items, onClose }: { items: { label: string; danger?: boolean; hidden?: boolean; run: () => void }[]; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    ref.current?.querySelector<HTMLButtonElement>('button')?.focus();
    const out = (e: MouseEvent) => { if (!ref.current?.contains(e.target as Node)) onClose(); };
    window.addEventListener('mousedown', out);
    return () => window.removeEventListener('mousedown', out);
  }, [onClose]);
  const shown = items.filter(i => !i.hidden);
  return (
    <div ref={ref} className="adm-doc-menu" role="menu" onKeyDown={e => {
      const btns = Array.from(ref.current?.querySelectorAll<HTMLButtonElement>('button') ?? []);
      const i = btns.indexOf(document.activeElement as HTMLButtonElement);
      if (e.key === 'Escape') { e.preventDefault(); onClose(); }
      if (e.key === 'ArrowDown') { e.preventDefault(); btns[(i + 1) % btns.length]?.focus(); }
      if (e.key === 'ArrowUp') { e.preventDefault(); btns[(i - 1 + btns.length) % btns.length]?.focus(); }
    }}>
      {shown.map(i => <button key={i.label} type="button" role="menuitem" className={i.danger ? 'is-danger' : ''} onClick={() => { onClose(); i.run(); }}>{i.label}</button>)}
    </div>
  );
}

function DocView({ id, people, onTreeChange, onAncestors }: { id: string; people: People; onTreeChange: () => void; onAncestors: (ids: string[]) => void }) {
  const toast = useToast();
  const confirm = useConfirm();
  const nav = useNavigate();
  const { me } = useStaff();
  const [detail, setDetail] = useState<DocDetail | null>(null);
  const [loadError, setLoadError] = useState<{ msg: string; status: number } | null>(null);
  const [title, setTitle] = useState('');
  const [icon, setIcon] = useState('');
  const [panel, setPanel] = useState<Panel>(null);
  const [anchor, setAnchor] = useState<CommentAnchor | null>(null);
  const [dialog, setDialog] = useState<'share' | 'move' | 'reject' | 'template' | null>(null);
  const [menu, setMenu] = useState(false);
  const [busy, setBusy] = useState(false);
  const [epoch, setEpoch] = useState(0);
  const [save, setSave] = useState<AutosaveSnapshot>({ state: 'idle', version: 0, savedAt: null, error: null, attempt: 0 });
  const editorRef = useRef<Editor | null>(null);
  const titleRef = useRef('');
  const iconRef = useRef<string | null>(null);
  const getContent = useRef<(() => DocNode) | null>(null);
  const ctrlRef = useRef<Autosave<() => DocSaveInput> | null>(null);
  const statusRef = useRef<string | null>(null);
  statusRef.current = detail?.doc.status ?? statusRef.current;

  const load = useCallback(async () => {
    try {
      const d = await workApi.doc(id);
      setDetail(d);
      setTitle(d.doc.title); titleRef.current = d.doc.title;
      setIcon(d.doc.icon ?? ''); iconRef.current = d.doc.icon;
      setLoadError(null);
      onAncestors(d.breadcrumb.map(b => b.id));
      return d;
    } catch (e) {
      setLoadError({ msg: errorMessage(e, '문서를 불러오지 못했어요.'), status: e instanceof ApiError ? e.status : 0 });
      return null;
    }
  }, [id, onAncestors]);

  useEffect(() => { setDetail(null); void load(); }, [load]);

  // 자동 저장 — 문서마다 하나
  const version = detail?.doc.version;
  const ready = detail != null;
  useEffect(() => {
    if (!ready || version == null) return;
    const ctrl = new Autosave<() => DocSaveInput>({
      version,
      save: async (build, base) => {
        const r = await workApi.saveDoc(id, { ...build(), base_version: base });
        // 승인·검토 중 문서를 고치면 초안이 된다 — 트리의 상태 표시도 바꾼다
        if (r.status !== statusRef.current) { statusRef.current = r.status; onTreeChange(); }
        setDetail(d => (d ? { ...d, doc: { ...d.doc, version: r.version, status: r.status, title: titleRef.current, icon: iconRef.current, updated_at: r.saved_at, updated_by: me.user_id } } : d));
        return r;
      },
      isConflict: e => e instanceof ApiError && e.status === 409,
    });
    ctrlRef.current = ctrl;
    const off = ctrl.subscribe(setSave);
    setSave(ctrl.snapshot);
    const leave = (e: BeforeUnloadEvent) => { if (ctrl.unsaved) { e.preventDefault(); void ctrl.flush(); } };
    window.addEventListener('beforeunload', leave);
    return () => {
      off();
      window.removeEventListener('beforeunload', leave);
      // 화면을 떠나도 마지막 변경은 보낸다
      void ctrl.flush().finally(() => ctrl.dispose());
      ctrlRef.current = null;
    };
    // 문서가 바뀌거나 내용을 갈아 끼울 때(epoch)만 새로 만든다
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id, ready, epoch]);

  const build = useCallback((): DocSaveInput => {
    const out: DocSaveInput = { base_version: 0, title: titleRef.current.trim() || '제목 없음', icon: iconRef.current || null };
    const content = getContent.current?.() ?? editorRef.current?.getJSON();
    if (content) out.content = normalizeDoc(content as DocNode);
    return out;
  }, []);

  const changed = useCallback((get: () => DocNode) => { getContent.current = get; ctrlRef.current?.change(build); }, [build]);
  const onReady = useCallback((e: Editor | null) => { editorRef.current = e; }, []);

  const level = detail?.doc.level ?? 'NONE';
  const archived = !!detail?.doc.archived_at;
  const canEdit = LEVEL_RANK[level] >= 2 && !archived;
  const canManage = LEVEL_RANK[level] >= 3;
  const isOwner = level === 'OWNER' || me.role === 'ADMIN';

  /** 저장을 끝내고 하는 일 (상태 변경·복원 전) */
  const settle = async () => { await ctrlRef.current?.flush(); };
  const replaceContent = (d: DocDetail) => {
    setDetail(d);
    editorRef.current?.commands.setContent(d.doc.content, { emitUpdate: false });
    setTitle(d.doc.title); titleRef.current = d.doc.title;
    setEpoch(x => x + 1);
  };

  const act = async (fn: () => Promise<unknown>, ok?: string) => {
    if (busy) return;
    setBusy(true);
    try { await fn(); if (ok) toast(ok, 'success'); } catch (e) { toast(errorMessage(e, '처리하지 못했어요.'), 'error'); } finally { setBusy(false); }
  };

  const status = (action: 'REQUEST_REVIEW' | 'APPROVE' | 'REJECT' | 'REOPEN', reason?: string) => act(async () => {
    await settle();
    const d = await workApi.docStatus(id, action, reason);
    setDetail(d);
    ctrlRef.current?.bump(d.doc.version);
    setEpoch(x => x + 1);
  }, action === 'REQUEST_REVIEW' ? '검토를 요청했어요.' : action === 'APPROVE' ? '승인했어요.' : action === 'REJECT' ? '반려했어요.' : '초안으로 되돌렸어요.');

  const restore = (v: number) => act(async () => {
    await settle();
    const d = await workApi.restoreDocVersion(id, v, ctrlRef.current?.version ?? detail?.doc.version ?? 0);
    replaceContent(d);
  }, `v${v}으로 되돌렸어요.`);

  const conflictReload = async () => {
    if (!(await confirm({ title: '최신본을 불러올까요?', message: '아직 저장되지 않은 내 변경은 사라져요. 필요하면 먼저 본문을 복사해 두세요.', confirmLabel: '불러오기', danger: true }))) return;
    const d = await load();
    if (d) replaceContent(d);
  };
  const conflictOverwrite = async () => {
    if (!(await confirm({ title: '내 내용으로 덮어쓸까요?', message: '다른 사람이 고친 내용은 버전 기록에 남고, 지금 화면의 내용이 최신본이 돼요.', confirmLabel: '덮어쓰기', danger: true }))) return;
    try {
      const d = await workApi.doc(id);
      const ctrl = ctrlRef.current;
      if (!ctrl) return;
      ctrl.reset(d.doc.version);
      ctrl.change(build);
      await ctrl.flush();
    } catch (e) { toast(errorMessage(e, '덮어쓰지 못했어요.'), 'error'); }
  };

  const reorder = (dir: -1 | 1) => act(async () => {
    if (!detail) return;
    const sibs = (await workApi.docTree(detail.doc.parent_id)).items;
    const i = sibs.findIndex(s => s.id === id);
    const j = i + dir;
    if (i < 0 || j < 0 || j >= sibs.length) return;
    // 자기를 뺀 목록에서 j번째 자리: 위는 rest[j-1], 아래는 rest[j]
    const rest = sibs.filter(s => s.id !== id);
    await workApi.moveDoc(id, { parent_id: detail.doc.parent_id, before_id: rest[j - 1]?.id ?? null, after_id: rest[j]?.id ?? null });
    onTreeChange();
  }, '순서를 바꿨어요.');

  const embeds = useMemo(() => new EmbedStore(keys => workApi.embeds(keys).then(r => r.items)), []);
  const ctx = useMemo<DocsCtx>(() => ({ embeds, people, editable: canEdit, inlineUrl: workApi.attachmentInline, fileUrl: workApi.attachmentUrl }), [embeds, people, canEdit]);

  if (loadError) {
    return loadError.status === 404
      ? <EmptyState title="문서를 찾을 수 없어요">삭제됐거나 볼 권한이 없는 문서예요.</EmptyState>
      : <ErrorBox message={loadError.msg} onRetry={() => void load()} />;
  }
  if (!detail) return <SkeletonList rows={6} />;
  const d = detail.doc;

  return (
    <DocsContext.Provider value={ctx}>
      <div className={`adm-doc-main${panel ? ' has-panel' : ''}`}>
        <div className="adm-doc-top">
          <nav className="adm-doc-crumbs" aria-label="문서 위치">
            {detail.breadcrumb.map(b => <span key={b.id}><button type="button" className="ui-btn is-text is-muted" onClick={() => nav(`/admin/docs/${b.id}`)}>{b.icon || '📄'} {b.title || '제목 없음'}</button> / </span>)}
            <span aria-current="page">{title || '제목 없음'}</span>
          </nav>
          <SaveStatus s={save} onRetry={() => ctrlRef.current?.retry()} />
          <Badge tone={DOC_STATUS[d.status][1]}>{DOC_STATUS[d.status][0]}</Badge>
          {d.staff_access == null && <Badge tone="gray"><Glyph name="lock" size={11} />제한됨</Badge>}
          <span className="adm-doc-top-actions">
            {!archived && d.status === 'DRAFT' && canEdit && <button type="button" className="ui-btn is-soft is-sm" disabled={busy} onClick={() => void status('REQUEST_REVIEW')}>검토 요청</button>}
            {!archived && d.status === 'IN_REVIEW' && canManage && (
              <>
                <button type="button" className="ui-btn is-primary is-sm" disabled={busy} onClick={() => void status('APPROVE')}>승인</button>
                <button type="button" className="ui-btn is-soft is-sm" disabled={busy} onClick={() => setDialog('reject')}>반려</button>
              </>
            )}
            {!archived && d.status !== 'DRAFT' && canEdit && <button type="button" className="ui-btn is-soft is-sm" disabled={busy} onClick={() => void status('REOPEN')}>초안으로</button>}
            {canManage && <button type="button" className="ui-btn is-soft is-sm" onClick={() => setDialog('share')}><Glyph name="users" size={13} />공유</button>}
            {(['comments', 'info', 'versions'] as const).map(p => (
              <button key={p} type="button" className={`ui-btn is-soft is-sm${panel === p ? ' is-on' : ''}`} aria-pressed={panel === p} onClick={() => setPanel(x => (x === p ? null : p))}>
                {p === 'comments' ? `댓글${detail.open_comments ? ` ${detail.open_comments}` : ''}` : p === 'info' ? '정보' : '기록'}
              </button>
            ))}
            <span className="adm-doc-more">
              <button type="button" className="ui-btn is-soft is-sm" aria-haspopup="menu" aria-expanded={menu} aria-label="더 보기" onClick={() => setMenu(m => !m)}>더 보기</button>
              {menu && (
                <MoreMenu onClose={() => setMenu(false)} items={[
                  { label: '복제', hidden: archived, run: () => void act(async () => { await settle(); const c = await workApi.duplicateDoc(id); onTreeChange(); nav(`/admin/docs/${c.doc.id}`); }, '복제했어요.') },
                  { label: '옮기기', hidden: !canManage || archived, run: () => setDialog('move') },
                  { label: '위로', hidden: !canManage || archived, run: () => void reorder(-1) },
                  { label: '아래로', hidden: !canManage || archived, run: () => void reorder(1) },
                  { label: '지금 버전 저장', hidden: !canEdit, run: () => void act(async () => { await settle(); await workApi.saveDocVersion(id); }, '버전을 저장했어요.') },
                  { label: '템플릿으로 저장', hidden: me.role !== 'ADMIN' || archived, run: () => setDialog('template') },
                  { label: '보관', hidden: !canManage || archived, danger: true, run: () => void (async () => {
                    if (await confirm({ title: '문서를 보관할까요?', message: '하위 문서도 함께 보관돼요. 보관함에서 복원할 수 있어요.', confirmLabel: '보관', danger: true })) {
                      await act(async () => { await settle(); await workApi.archiveDoc(id); onTreeChange(); await load(); }, '보관했어요.');
                    }
                  })() },
                  { label: '복원', hidden: !archived || !canManage, run: () => void act(async () => { const r = await workApi.restoreDoc(id); setDetail(r); onTreeChange(); setEpoch(x => x + 1); }, '복원했어요.') },
                  { label: '영구 삭제', hidden: !archived || !isOwner, danger: true, run: () => void (async () => {
                    if (await confirm({ title: '문서를 삭제할까요?', message: '하위 문서까지 목록에서 사라지고 되돌릴 수 없어요. 감사 기록은 남아요.', confirmLabel: '삭제', danger: true })) {
                      await act(async () => { await workApi.deleteDoc(id); onTreeChange(); nav('/admin/docs'); }, '삭제했어요.');
                    }
                  })() },
                ]} />
              )}
            </span>
          </span>
        </div>

        {save.state === 'conflict' && (
          <div className="ui-callout is-warn adm-doc-conflict" role="alert">
            다른 사람이 이 문서를 먼저 고쳤어요. 내 변경은 아직 저장되지 않았어요.
            <span className="adm-w-btns">
              <button type="button" className="ui-btn is-soft is-sm" onClick={() => void conflictReload()}>최신본 불러오기</button>
              <button type="button" className="ui-btn is-soft is-sm" onClick={() => void conflictOverwrite()}>내 내용으로 덮어쓰기</button>
            </span>
          </div>
        )}
        {archived && <div className="ui-callout">보관된 문서예요. 읽기만 할 수 있어요.</div>}
        {d.status === 'APPROVED' && canEdit && <div className="ui-callout small">승인된 문서예요. 고치면 다시 초안이 되고 검토를 받아야 해요.</div>}
        {d.review.rejection_reason && d.status === 'DRAFT' && <div className="ui-callout is-warn small">반려 사유: {d.review.rejection_reason}</div>}

        <div className="adm-doc-head">
          <label className="sr-only" htmlFor="doc-icon">아이콘</label>
          <input id="doc-icon" className="adm-doc-icon" value={icon} placeholder="📄" maxLength={8} disabled={!canEdit}
            onChange={e => { const v = e.target.value.trim(); setIcon(v); iconRef.current = v || null; ctrlRef.current?.change(build); }} />
          <label className="sr-only" htmlFor="doc-title">제목</label>
          <input id="doc-title" className="adm-doc-title" value={title} placeholder="제목 없음" maxLength={200} readOnly={!canEdit}
            onChange={e => { setTitle(e.target.value); titleRef.current = e.target.value; ctrlRef.current?.change(build); }}
            onKeyDown={e => { if (e.key === 'Enter' || e.key === 'ArrowDown') { e.preventDefault(); editorRef.current?.commands.focus('start'); } }} />
        </div>

        <div className="adm-doc-body">
          <DocEditor key={`${id}:${epoch}`} docId={id} content={d.content} editable={canEdit} people={people}
            onChange={changed} onReady={onReady} onComment={a => { setAnchor(a); setPanel('comments'); }} />
          {panel && (
            <aside className="adm-doc-side" aria-label="문서 패널">
              <button type="button" className="ui-btn is-text is-muted adm-doc-side-close" onClick={() => setPanel(null)}>닫기</button>
              {panel === 'comments' && (
                <CommentsPanel docId={id} people={people} canWrite={!archived} anchor={anchor} onClearAnchor={() => setAnchor(null)}
                  onChanged={() => void workApi.doc(id).then(r => setDetail(x => (x ? { ...x, open_comments: r.open_comments } : x))).catch(() => {})}
                  onJump={a => {
                    const ed = editorRef.current;
                    if (!ed || a.from == null || a.to == null) return;
                    const max = ed.state.doc.content.size;
                    ed.chain().focus().setTextSelection({ from: Math.min(a.from, max), to: Math.min(a.to, max) }).scrollIntoView().run();
                  }} />
              )}
              {panel === 'info' && (
                <InfoPanel detail={detail} people={people} busy={busy} onNewCard={() => void act(async () => {
                  await workApi.createTask({ title: titleRef.current || '문서 후속 작업', type: 'GENERAL', priority: 'NORMAL', links: [{ entity_kind: 'doc', entity_id: id }] });
                  await load();
                }, '카드를 만들었어요.')} />
              )}
              {panel === 'versions' && (
                <VersionsPanel docId={id} people={people} canRestore={canEdit} onRestore={v => void restore(v)}
                  current={() => (editorRef.current?.getJSON() as DocNode | undefined) ?? d.content} />
              )}
            </aside>
          )}
        </div>
      </div>
      {dialog === 'share' && <ShareDialog docId={id} people={people} onClose={() => setDialog(null)} onSaved={() => { void load(); onTreeChange(); }} />}
      {dialog === 'move' && <MoveDialog docId={id} onClose={() => setDialog(null)} onMove={parent => { setDialog(null); void act(async () => { await settle(); const r = await workApi.moveDoc(id, { parent_id: parent }); setDetail(r); onTreeChange(); onAncestors(r.breadcrumb.map(b => b.id)); }, '옮겼어요.'); }} />}
      {dialog === 'reject' && <ReasonDialog title="반려" label="반려 사유 (작성자에게 보여요)" onClose={() => setDialog(null)} onSubmit={r => { setDialog(null); void status('REJECT', r); }} />}
      {dialog === 'template' && <TemplateDialog docId={id} defaultName={title} onClose={() => setDialog(null)} />}
    </DocsContext.Provider>
  );
}

export function Docs() {
  const { id } = useParams<{ id: string }>();
  const nav = useNavigate();
  const toast = useToast();
  const people = usePeople();
  const [params, setParams] = useSearchParams();
  const q = params.get('q') ?? '';
  const [query, setQuery] = useState(q);
  const [archived, setArchived] = useState(false);
  const [reloadKey, setReloadKey] = useState(0);
  const [ancestors, setAncestors] = useState<string[]>([]);
  const [creating, setCreating] = useState<{ parent: string | null } | null>(null);
  const [treeOpen, setTreeOpen] = useState(true);
  useEffect(() => {
    const t = window.setTimeout(() => { if (query !== q) setParams(query ? { q: query } : {}, { replace: true }); }, 300);
    return () => window.clearTimeout(t);
  }, [query, q, setParams]);

  const treeChanged = useCallback(() => setReloadKey(k => k + 1), []);
  const open = useCallback((docId: string) => { nav(`/admin/docs/${docId}`); if (window.matchMedia('(max-width: 860px)').matches) setTreeOpen(false); }, [nav]);

  const create = async (template: DocTemplate | null, title: string) => {
    const parent = creating?.parent ?? null;
    setCreating(null);
    try {
      const d = await workApi.createDoc({ parent_id: parent, template_id: template?.id ?? null, title: title.trim() || template?.name || '' });
      treeChanged();
      nav(`/admin/docs/${d.doc.id}`);
    } catch (e) {
      toast(errorMessage(e, '문서를 만들지 못했어요.'), 'error');
    }
  };

  return (
    <div className={`adm-docs${treeOpen ? '' : ' is-tree-closed'}`}>
      <aside className="adm-docs-nav" aria-label="문서 목록">
        <div className="adm-docs-nav-top">
          <h1>문서</h1>
          <button type="button" className="ui-btn is-primary is-sm" onClick={() => setCreating({ parent: null })}><Glyph name="plus" size={13} />새 문서</button>
        </div>
        <label className="sr-only" htmlFor="doc-search">문서 찾기</label>
        <input id="doc-search" className="ui-input" type="search" placeholder="제목·본문으로 찾기" value={query} onChange={e => setQuery(e.target.value)} />
        <div className="adm-docs-nav-body">
          {q ? <DocSearchResults q={q} onOpen={() => setQuery('')} />
            : <DocTree currentId={id ?? null} reloadKey={reloadKey} ancestors={ancestors} archived={archived} onOpen={open} onAddChild={p => setCreating({ parent: p })} />}
        </div>
        <button type="button" className="ui-btn is-text is-muted adm-docs-archive" aria-pressed={archived} onClick={() => setArchived(a => !a)}>
          {archived ? '← 문서로 돌아가기' : '보관함'}
        </button>
      </aside>
      <button type="button" className="adm-docs-nav-toggle" aria-expanded={treeOpen} onClick={() => setTreeOpen(o => !o)}>{treeOpen ? '목록 닫기' : '문서 목록'}</button>
      <main className="adm-docs-content">
        {id ? <DocView key={id} id={id} people={people} onTreeChange={treeChanged} onAncestors={setAncestors} /> : (
          <EmptyState title="문서를 고르거나 새로 만드세요" icon={<span aria-hidden="true">📄</span>}>
            회의록·장애 보고·DSP 이슈·권리 검토 같은 템플릿으로 시작할 수 있어요. 본문에서 ‘/’를 누르면 발매·QC·DSP·카드 같은 AUDENIQ 블록을 넣을 수 있어요.
            <p><button type="button" className="ui-btn is-primary" onClick={() => setCreating({ parent: null })}>새 문서</button></p>
          </EmptyState>
        )}
      </main>
      {creating && <NewDocDialog parentId={creating.parent} onClose={() => setCreating(null)} onCreate={(t, title) => void create(t, title)} />}
    </div>
  );
}
