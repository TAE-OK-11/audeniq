// 문서 편집기 — Tiptap(ProseMirror) 위에 AUDENIQ 블록·'/' 명령·'@' 멘션·파일/이미지·링크 미리보기.
// 입력은 편집기 안에서만 처리하고(React 상태로 옮기지 않음), 자동 저장은 Autosave가 묶어서 보낸다.
// 저장 직전에만 JSON을 꺼내 서버 허용 모양으로 정리한다(normalizeDoc). 보기 권한이면 읽기 전용.
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { EditorContent, useEditor, useEditorState, type Editor } from '@tiptap/react';
import type { Range } from '@tiptap/core';
import StarterKit from '@tiptap/starter-kit';
import { TaskItem, TaskList } from '@tiptap/extension-list';
import { Table, TableCell, TableHeader, TableRow } from '@tiptap/extension-table';
import { Details, DetailsContent, DetailsSummary } from '@tiptap/extension-details';
import { Placeholder } from '@tiptap/extensions';
import { useToast } from '../../../components/Toast';
import { Modal } from '../../../components/Modal';
import { errorMessage } from '../../../api/errors';
import { Glyph } from '../../../components/Glyph';
import { workApi } from '../api';
import { EntityPicker, type People } from '../ui';
import type { DocNode, EntityKind, EntityRef } from '../types';
import {
  Callout, DocImage, EntityEmbed, FileAttachment, LinkPreview, Mention, MentionSuggest, SlashCommands,
  type SuggestBridge, type SuggestState,
} from './extensions';
import { filterSlash, type SlashItem } from './slash';
import { isSafeUrl } from './model';

const IMAGE_TYPES = ['image/png', 'image/jpeg', 'image/webp'];
const MAX_FILE = 20 * 1024 * 1024;

export interface MentionItem { kind: 'user' | 'doc'; id: string; label: string; sub?: string }
export interface CommentAnchor { from: number; to: number; quote: string }

type Pending =
  | { kind: 'embed'; item: SlashItem; at: number }
  | { kind: 'link'; at: number }
  | { kind: 'mentionDoc'; at: number };

/** 추천 창 — '/' 명령, '@' 멘션 공용. 위·아래·Enter·Esc, 마우스 */
const SuggestMenu = memo(function SuggestMenu({ state, active, onPick, onHover }: {
  state: SuggestState<unknown>; active: number; onPick: (i: number) => void; onHover: (i: number) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => { ref.current?.querySelector(`[data-i="${active}"]`)?.scrollIntoView({ block: 'nearest' }); }, [active]);
  const r = state.rect;
  const style = r ? { left: Math.min(r.left, window.innerWidth - 300), top: r.bottom + 6 } : undefined;
  const label = state.kind === 'slash' ? '넣을 블록' : '언급할 사람·문서';
  if (state.items.length === 0) {
    return <div className="adm-doc-suggest is-empty" style={style} role="status">{state.kind === 'slash' ? '맞는 명령이 없어요' : '찾은 사람·문서가 없어요'}</div>;
  }
  let group = '';
  return (
    <div ref={ref} className="adm-doc-suggest" style={style} role="listbox" aria-label={label} id="doc-suggest">
      {state.items.map((raw, i) => {
        if (state.kind === 'slash') {
          const it = raw as SlashItem;
          const head = it.group !== group ? (group = it.group) : null;
          return (
            <div key={it.id}>
              {head && <div className="adm-doc-suggest-group" role="presentation">{head}</div>}
              <button type="button" role="option" id={`doc-suggest-${i}`} data-i={i} aria-selected={i === active}
                className={i === active ? 'is-active' : ''} onMouseDown={e => { e.preventDefault(); onPick(i); }} onMouseEnter={() => onHover(i)}>
                <span className="adm-doc-suggest-icon" aria-hidden="true">{it.icon.startsWith('g:') ? <Glyph name={it.icon.slice(2)} size={16} /> : it.icon}</span>
                <span className="adm-min"><b>{it.title}</b><small>{it.hint}</small></span>
                <kbd>/{it.id}</kbd>
              </button>
            </div>
          );
        }
        const it = raw as MentionItem;
        return (
          <button key={`${it.kind}:${it.id}`} type="button" role="option" id={`doc-suggest-${i}`} data-i={i} aria-selected={i === active}
            className={i === active ? 'is-active' : ''} onMouseDown={e => { e.preventDefault(); onPick(i); }} onMouseEnter={() => onHover(i)}>
            <span className="adm-doc-suggest-icon" aria-hidden="true">{it.kind === 'doc' ? <Glyph name="doc" size={15} /> : <Glyph name="at" size={15} />}</span>
            <span className="adm-min"><b>{it.label}</b>{it.sub && <small>{it.sub}</small>}</span>
          </button>
        );
      })}
    </div>
  );
});

function ToolButton({ label, on, disabled, onClick, children }: { label: string; on?: boolean; disabled?: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <button type="button" className={`adm-doc-tool${on ? ' is-on' : ''}`} aria-label={label} title={label} aria-pressed={on} disabled={disabled}
      onMouseDown={e => e.preventDefault()} onClick={onClick}>{children}</button>
  );
}

/** 서식 줄 — 편집기 상태가 바뀐 부분만 다시 그린다(useEditorState) */
function Toolbar({ editor, onLink, onComment, onSlash }: { editor: Editor; onLink: () => void; onComment: () => void; onSlash: () => void }) {
  const st = useEditorState({
    editor,
    selector: ({ editor: e }) => ({
      bold: e.isActive('bold'), italic: e.isActive('italic'), underline: e.isActive('underline'), strike: e.isActive('strike'),
      code: e.isActive('code'), link: e.isActive('link'), h1: e.isActive('heading', { level: 1 }), h2: e.isActive('heading', { level: 2 }),
      bullet: e.isActive('bulletList'), ordered: e.isActive('orderedList'), task: e.isActive('taskList'),
      canUndo: e.can().undo(), canRedo: e.can().redo(), empty: e.state.selection.empty,
    }),
  });
  const c = () => editor.chain().focus();
  return (
    <div className="adm-doc-toolbar" role="toolbar" aria-label="서식">
      <ToolButton label="실행 취소 (Ctrl+Z)" disabled={!st.canUndo} onClick={() => c().undo().run()}><Glyph name="undo" size={15} /></ToolButton>
      <ToolButton label="다시 실행 (Ctrl+Shift+Z)" disabled={!st.canRedo} onClick={() => c().redo().run()}><Glyph name="redo" size={15} /></ToolButton>
      <span className="adm-doc-tool-sep" aria-hidden="true" />
      <ToolButton label="제목 1" on={st.h1} onClick={() => c().toggleHeading({ level: 1 }).run()}>H1</ToolButton>
      <ToolButton label="제목 2" on={st.h2} onClick={() => c().toggleHeading({ level: 2 }).run()}>H2</ToolButton>
      <ToolButton label="굵게 (Ctrl+B)" on={st.bold} onClick={() => c().toggleBold().run()}><b>B</b></ToolButton>
      <ToolButton label="기울임 (Ctrl+I)" on={st.italic} onClick={() => c().toggleItalic().run()}><i>I</i></ToolButton>
      <ToolButton label="밑줄 (Ctrl+U)" on={st.underline} onClick={() => c().toggleUnderline().run()}><u>U</u></ToolButton>
      <ToolButton label="취소선" on={st.strike} onClick={() => c().toggleStrike().run()}><s>S</s></ToolButton>
      <ToolButton label="코드" on={st.code} onClick={() => c().toggleCode().run()}>{'<>'}</ToolButton>
      <ToolButton label="링크 (Ctrl+K)" on={st.link} onClick={onLink}><Glyph name="link" size={15} /></ToolButton>
      <span className="adm-doc-tool-sep" aria-hidden="true" />
      <ToolButton label="글머리 목록" on={st.bullet} onClick={() => c().toggleBulletList().run()}><Glyph name="list" size={15} /></ToolButton>
      <ToolButton label="번호 목록" on={st.ordered} onClick={() => c().toggleOrderedList().run()}>1.</ToolButton>
      <ToolButton label="할 일 목록" on={st.task} onClick={() => c().toggleTaskList().run()}><Glyph name="checklist" size={15} /></ToolButton>
      <ToolButton label="블록 넣기 (/)" onClick={onSlash}><Glyph name="plus" size={14} /></ToolButton>
      <span className="adm-doc-tool-sep" aria-hidden="true" />
      <ToolButton label="선택한 부분에 댓글" disabled={st.empty} onClick={onComment}><Glyph name="inquiry" size={14} /></ToolButton>
    </div>
  );
}

function LinkDialog({ initial, card, onClose, onSubmit }: { initial: string; card: boolean; onClose: () => void; onSubmit: (v: { url: string; title: string; description: string }) => void }) {
  const [url, setUrl] = useState(initial);
  const [title, setTitle] = useState('');
  const [description, setDescription] = useState('');
  const ok = isSafeUrl(url) && (!card || !url.toLowerCase().startsWith('mailto:'));
  return (
    <Modal title={card ? '링크 미리보기 넣기' : '링크'} onClose={onClose}>
      <form className="adm-form" onSubmit={e => { e.preventDefault(); if (ok || (!card && !url)) onSubmit({ url: url.trim(), title: title.trim(), description: description.trim() }); }}>
        <label className="ui-field"><span>주소</span>
          <input className="ui-input" autoFocus type="url" inputMode="url" placeholder="https://" value={url} onChange={e => setUrl(e.target.value)} maxLength={2048} />
          {url && !ok && <small className="adm-field-error">http(s){card ? '' : ' 또는 mailto'} 주소만 넣을 수 있어요.</small>}
        </label>
        {card && (
          <>
            <label className="ui-field"><span>제목</span><input className="ui-input" value={title} maxLength={300} onChange={e => setTitle(e.target.value)} placeholder="비워 두면 주소를 보여 줘요" /></label>
            <label className="ui-field"><span>설명</span><input className="ui-input" value={description} maxLength={300} onChange={e => setDescription(e.target.value)} /></label>
            <p className="small muted">보안을 위해 서버가 주소를 대신 열어 보지 않아요. 제목·설명은 직접 적어 주세요.</p>
          </>
        )}
        <div className="adm-form-actions">
          {!card && initial && <button type="button" className="ui-btn is-soft" onClick={() => onSubmit({ url: '', title: '', description: '' })}>링크 없애기</button>}
          <button type="button" className="ui-btn is-soft" onClick={onClose}>취소</button>
          <button type="submit" className="ui-btn is-primary" disabled={!ok}>넣기</button>
        </div>
      </form>
    </Modal>
  );
}

const PICK_TITLE: Partial<Record<EntityKind, string>> = { release: '발매 고르기', track: '트랙 고르기', artist: '아티스트 고르기', dsp: 'DSP 고르기', inquiry: '문의 고르기', task: '카드 고르기', approval: '2차 승인 고르기', doc: '문서 고르기' };

export interface DocEditorProps {
  docId: string;
  content: DocNode;
  editable: boolean;
  people: People;
  /** 내용이 바뀌었다 — 저장은 부모(Autosave)가 */
  onChange: (getContent: () => DocNode) => void;
  onComment: (a: CommentAnchor) => void;
  /** 편집기 핸들 (버전 복원·충돌 해결 때 내용 갈아 끼우기) */
  onReady?: (e: Editor | null) => void;
}

export const DocEditor = memo(function DocEditor({ docId, content, editable, people, onChange, onComment, onReady }: DocEditorProps) {
  const toast = useToast();
  const [suggest, setSuggest] = useState<SuggestState<unknown> | null>(null);
  const [active, setActive] = useState(0);
  const [pending, setPending] = useState<Pending | null>(null);
  const [linkEdit, setLinkEdit] = useState<string | null>(null);
  const [uploading, setUploading] = useState<string | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const uploadAt = useRef<{ at: number; as: 'image' | 'file' } | null>(null);
  const suggestRef = useRef<{ s: SuggestState<unknown> | null; active: number }>({ s: null, active: 0 });
  const changeRef = useRef(onChange);
  changeRef.current = onChange;
  const peopleRef = useRef(people);
  peopleRef.current = people;
  const editorRef = useRef<Editor | null>(null);

  // 추천 창 ↔ 편집기 키 입력 (편집기 확장은 한 번만 만들므로 ref로 잇는다)
  const bridge = useMemo<SuggestBridge>(() => ({
    set: s => {
      suggestRef.current = { s, active: s && suggestRef.current.s?.query === s.query ? suggestRef.current.active : 0 };
      setSuggest(s);
      setActive(suggestRef.current.active);
    },
    key: e => {
      const cur = suggestRef.current;
      if (!cur.s) return false;
      const len = cur.s.items.length;
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        if (!len) return true;
        const next = (cur.active + (e.key === 'ArrowDown' ? 1 : len - 1)) % len;
        suggestRef.current = { ...cur, active: next };
        setActive(next);
        return true;
      }
      if (e.key === 'Enter' || e.key === 'Tab') {
        const it = cur.s.items[cur.active];
        if (it === undefined) return false;
        cur.s.command(it);
        return true;
      }
      if (e.key === 'Escape') { setSuggest(null); suggestRef.current = { s: null, active: 0 }; return true; }
      return false;
    },
  }), []);

  const upload = useCallback(async (files: File[], at: number, as: 'image' | 'file' | 'auto') => {
    const ed = editorRef.current;
    if (!ed) return;
    let pos = at;
    for (const f of files) {
      if (f.size > MAX_FILE) { toast(`${f.name}: 파일은 20MB까지 넣을 수 있어요.`, 'error'); continue; }
      const image = as === 'image' || (as === 'auto' && IMAGE_TYPES.includes(f.type));
      if (image && !IMAGE_TYPES.includes(f.type)) { toast(`${f.name}: 이미지는 PNG·JPG·WebP만 넣을 수 있어요.`, 'error'); continue; }
      setUploading(f.name);
      try {
        const id = await workApi.upload(f);
        const node = image
          ? { type: 'docImage', attrs: { attachmentId: id, alt: f.name.replace(/\.[^.]+$/, '').slice(0, 300) } }
          : { type: 'fileAttachment', attrs: { attachmentId: id, name: f.name.slice(0, 200), size: f.size } };
        ed.chain().focus().insertContentAt(Math.min(pos, ed.state.doc.content.size), node).run();
        pos = ed.state.selection.to;
      } catch (e) {
        toast(errorMessage(e, `${f.name}을(를) 올리지 못했어요.`), 'error');
      } finally {
        setUploading(null);
      }
    }
  }, [toast]);

  const runSlash = useCallback((ed: Editor, range: Range, item: SlashItem) => {
    const ch = ed.chain().focus().deleteRange(range);
    const a = item.action;
    if (a.kind === 'block') {
      switch (a.block) {
        case 'text': ch.setParagraph().run(); break;
        case 'h1': ch.setHeading({ level: 1 }).run(); break;
        case 'h2': ch.setHeading({ level: 2 }).run(); break;
        case 'h3': ch.setHeading({ level: 3 }).run(); break;
        case 'todo': ch.toggleTaskList().run(); break;
        case 'list': ch.toggleBulletList().run(); break;
        case 'ordered': ch.toggleOrderedList().run(); break;
        case 'quote': ch.toggleBlockquote().run(); break;
        case 'callout': ch.setCallout('info').run(); break;
        case 'divider': ch.setHorizontalRule().run(); break;
        case 'toggle': ch.setDetails().run(); break;
        case 'code': ch.toggleCodeBlock().run(); break;
        case 'table': ch.insertTable({ rows: 3, cols: 3, withHeaderRow: true }).run(); break;
      }
      return;
    }
    ch.run();
    const at = ed.state.selection.from;
    if (a.kind === 'upload') {
      uploadAt.current = { at, as: a.as };
      if (fileInput.current) {
        fileInput.current.accept = a.as === 'image' ? IMAGE_TYPES.join(',') : '';
        fileInput.current.click();
      }
    } else if (a.kind === 'link') setPending({ kind: 'link', at });
    else if (a.kind === 'mentionDoc') setPending({ kind: 'mentionDoc', at });
    else setPending({ kind: 'embed', item, at });
  }, []);

  const mentionItems = useCallback(async (q: string): Promise<MentionItem[]> => {
    const term = q.toLowerCase();
    const users: MentionItem[] = peopleRef.current.list
      .filter(p => !term || p.name.toLowerCase().includes(term) || p.email.toLowerCase().startsWith(term))
      .slice(0, 5).map(p => ({ kind: 'user', id: p.user_id, label: p.name, sub: p.email }));
    let docs: MentionItem[] = [];
    if (term.length >= 1) {
      try {
        const r = await workApi.searchEntities('doc', q);
        docs = r.items.filter(d => d.id !== docId && !d.missing).slice(0, 5).map(d => ({ kind: 'doc', id: d.id, label: d.label ?? '문서', sub: '문서' }));
      } catch { /* 사람만 보여 준다 */ }
    }
    return [...users, ...docs];
  }, [docId]);

  const extensions = useMemo(() => [
    StarterKit.configure({
      heading: { levels: [1, 2, 3] },
      link: {
        openOnClick: false, autolink: true, defaultProtocol: 'https', protocols: ['http', 'https', 'mailto'],
        isAllowedUri: url => isSafeUrl(url),
        HTMLAttributes: { rel: 'noopener noreferrer nofollow', target: '_blank' },
      },
    }),
    TaskList, TaskItem.configure({ nested: true }),
    Table.configure({ resizable: false }), TableRow, TableHeader, TableCell,
    Details, DetailsSummary, DetailsContent,
    Placeholder.configure({ placeholder: ({ node }) => (node.type.name === 'heading' ? '제목' : "'/'로 블록 넣기, '@'로 사람·문서 언급") }),
    Callout, DocImage, FileAttachment, LinkPreview, EntityEmbed, Mention,
    SlashCommands<SlashItem>({ items: q => filterSlash(q), run: runSlash, bridge }),
    MentionSuggest<MentionItem>({
      items: mentionItems,
      run: (ed, range, it) => {
        ed.chain().focus().deleteRange(range).insertContent([{ type: 'mention', attrs: { kind: it.kind, id: it.id, label: it.label } }, { type: 'text', text: ' ' }]).run();
      },
      bridge,
    }),
  ], [bridge, runSlash, mentionItems]);

  const editor = useEditor({
    extensions,
    content,
    editable,
    immediatelyRender: true,
    shouldRerenderOnTransaction: false,
    editorProps: {
      attributes: { class: 'adm-doc-prose', role: 'textbox', 'aria-multiline': 'true', 'aria-label': '문서 본문' },
      handlePaste: (view, e) => {
        const files = Array.from(e.clipboardData?.files ?? []);
        if (!files.length || !view.editable) return false;
        e.preventDefault();
        void upload(files, view.state.selection.from, 'auto');
        return true;
      },
      handleDrop: (view, e) => {
        const files = Array.from(e.dataTransfer?.files ?? []);
        if (!files.length || !view.editable) return false;
        e.preventDefault();
        const at = view.posAtCoords({ left: e.clientX, top: e.clientY })?.pos ?? view.state.selection.from;
        void upload(files, at, 'auto');
        return true;
      },
      handleKeyDown: (_view, e) => {
        if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
          e.preventDefault();
          setLinkEdit(String(editorRef.current?.getAttributes('link').href ?? ''));
          return true;
        }
        return false;
      },
    },
    onUpdate: ({ editor: ed }) => changeRef.current(() => ed.getJSON() as DocNode),
  }, [docId]);

  useEffect(() => { editorRef.current = editor; onReady?.(editor); return () => onReady?.(null); }, [editor, onReady]);
  useEffect(() => { if (editor && editor.isEditable !== editable) editor.setEditable(editable); }, [editor, editable]);

  const pick = (r: EntityRef) => {
    const ed = editorRef.current;
    const p = pending;
    setPending(null);
    if (!ed || !p) return;
    if (p.kind === 'mentionDoc') {
      ed.chain().focus().insertContentAt(p.at, [{ type: 'mention', attrs: { kind: 'doc', id: r.id, label: r.label ?? '문서' } }, { type: 'text', text: ' ' }]).run();
      return;
    }
    if (p.kind !== 'embed' || p.item.action.kind !== 'embed') return;
    const a = p.item.action;
    ed.chain().focus().insertContentAt(p.at, {
      type: 'entityEmbed',
      attrs: { kind: a.embed, id: r.id, view: a.view ?? 'summary', label: (r.label ?? '').slice(0, 200) },
    }).run();
  };

  const comment = () => {
    const ed = editorRef.current;
    if (!ed) return;
    const { from, to } = ed.state.selection;
    if (from === to) return;
    onComment({ from, to, quote: ed.state.doc.textBetween(from, to, ' ').slice(0, 200) });
  };

  const setLink = ({ url }: { url: string }) => {
    const ed = editorRef.current;
    setLinkEdit(null);
    if (!ed) return;
    if (!url) ed.chain().focus().extendMarkRange('link').unsetLink().run();
    else ed.chain().focus().extendMarkRange('link').setLink({ href: url }).run();
  };

  if (!editor) return <div className="ui-skel-rows"><span /><span /></div>;
  return (
    <div className={`adm-doc-editor${editable ? '' : ' is-readonly'}`}>
      {editable && (
        <Toolbar editor={editor} onComment={comment}
          onLink={() => setLinkEdit(String(editor.getAttributes('link').href ?? ''))}
          onSlash={() => editor.chain().focus().insertContent('/').run()} />
      )}
      <EditorContent editor={editor} />
      {uploading && <p className="adm-doc-uploading" role="status">{uploading} 올리는 중…</p>}
      {suggest && <SuggestMenu state={suggest} active={active} onHover={i => { suggestRef.current.active = i; setActive(i); }}
        onPick={i => { const it = suggest.items[i]; if (it !== undefined) suggest.command(it); }} />}
      <div className="sr-only" aria-live="polite">{suggest ? `${suggest.items.length}개 항목, 위아래 화살표로 고르고 Enter` : ''}</div>
      <input ref={fileInput} type="file" hidden multiple onChange={e => {
        const files = Array.from(e.target.files ?? []);
        const at = uploadAt.current;
        e.target.value = '';
        if (files.length && at) void upload(files, at.at, at.as);
      }} />
      {pending && (pending.kind === 'embed' || pending.kind === 'mentionDoc') && (
        <Modal title={pending.kind === 'mentionDoc' ? '문서 고르기' : PICK_TITLE[pending.item.action.kind === 'embed' ? pending.item.action.pick : 'release'] ?? '고르기'} onClose={() => setPending(null)}>
          <EntityPicker kinds={[pending.kind === 'mentionDoc' ? 'doc' : pending.item.action.kind === 'embed' ? pending.item.action.pick : 'release']} onPick={pick}
            exclude={pending.kind === 'mentionDoc' ? [`doc:${docId}`] : []} />
        </Modal>
      )}
      {pending?.kind === 'link' && (
        <LinkDialog initial="" card onClose={() => setPending(null)} onSubmit={v => {
          const at = pending.at;
          setPending(null);
          editor.chain().focus().insertContentAt(at, { type: 'linkPreview', attrs: { url: v.url, title: v.title || null, description: v.description || null } }).run();
        }} />
      )}
      {linkEdit != null && <LinkDialog initial={linkEdit} card={false} onClose={() => setLinkEdit(null)} onSubmit={setLink} />}
    </div>
  );
});
