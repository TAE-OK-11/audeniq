// AUDENIQ 문서 편집기 확장 — 서버 허용 목록(doc_model.rs)과 같은 이름·속성의 노드만 만든다.
// callout · docImage · fileAttachment · linkPreview · entityEmbed(실시간 값) · mention(@사람/@문서),
// 그리고 '/' 명령과 '@' 추천(@tiptap/suggestion). 추천 창은 React 쪽(DocEditor)이 그린다(SuggestBridge).
import { Extension, Node, mergeAttributes, type Editor, type Range } from '@tiptap/core';
import { ReactNodeViewRenderer } from '@tiptap/react';
import { PluginKey } from '@tiptap/pm/state';
import Suggestion, { type SuggestionKeyDownProps, type SuggestionProps } from '@tiptap/suggestion';
import { CalloutView, EmbedView, FileView, ImageView, LinkPreviewView } from './nodeviews';

export type CalloutTone = 'info' | 'warning' | 'danger' | 'success';

declare module '@tiptap/core' {
  interface Commands<ReturnType> {
    callout: { setCallout: (tone?: CalloutTone) => ReturnType };
  }
}

const str = (name: string) => ({
  default: null as string | null,
  parseHTML: (el: HTMLElement) => el.getAttribute(`data-${name}`),
  renderHTML: (attrs: Record<string, unknown>) => (attrs[name] == null ? {} : { [`data-${name}`]: String(attrs[name]) }),
});
const num = (name: string) => ({
  default: null as number | null,
  parseHTML: (el: HTMLElement) => { const v = Number(el.getAttribute(`data-${name}`)); return Number.isFinite(v) && v > 0 ? Math.round(v) : null; },
  renderHTML: (attrs: Record<string, unknown>) => (attrs[name] == null ? {} : { [`data-${name}`]: String(attrs[name]) }),
});

export const Callout = Node.create({
  name: 'callout',
  group: 'block',
  content: 'block+',
  defining: true,
  addAttributes() {
    return {
      tone: {
        default: 'info',
        parseHTML: el => { const t = el.getAttribute('data-tone'); return t === 'warning' || t === 'danger' || t === 'success' ? t : 'info'; },
        renderHTML: attrs => ({ 'data-tone': String(attrs.tone ?? 'info') }),
      },
    };
  },
  parseHTML() { return [{ tag: 'div[data-type="callout"]' }]; },
  renderHTML({ HTMLAttributes }) { return ['div', mergeAttributes(HTMLAttributes, { 'data-type': 'callout' }), 0]; },
  addNodeView() { return ReactNodeViewRenderer(CalloutView); },
  addCommands() {
    return {
      setCallout: (tone = 'info') => ({ commands }) => commands.wrapIn(this.name, { tone }),
    };
  },
});

const atomBlock = { group: 'block', atom: true, selectable: true, draggable: true } as const;

export const DocImage = Node.create({
  name: 'docImage',
  ...atomBlock,
  addAttributes() { return { attachmentId: str('attachment-id'), alt: str('alt'), width: num('width') }; },
  parseHTML() { return [{ tag: 'figure[data-type="doc-image"]' }]; },
  renderHTML({ HTMLAttributes }) { return ['figure', mergeAttributes(HTMLAttributes, { 'data-type': 'doc-image' })]; },
  addNodeView() { return ReactNodeViewRenderer(ImageView); },
});

export const FileAttachment = Node.create({
  name: 'fileAttachment',
  ...atomBlock,
  addAttributes() { return { attachmentId: str('attachment-id'), name: str('name'), size: num('size') }; },
  parseHTML() { return [{ tag: 'div[data-type="file-attachment"]' }]; },
  renderHTML({ HTMLAttributes }) { return ['div', mergeAttributes(HTMLAttributes, { 'data-type': 'file-attachment' })]; },
  addNodeView() { return ReactNodeViewRenderer(FileView); },
});

/** 링크 미리보기 — 서버가 주소를 대신 열지 않는다(SSRF 방지): 제목·설명은 쓴 사람이 적는다 */
export const LinkPreview = Node.create({
  name: 'linkPreview',
  ...atomBlock,
  addAttributes() { return { url: str('url'), title: str('title'), description: str('description') }; },
  parseHTML() { return [{ tag: 'div[data-type="link-preview"]' }]; },
  renderHTML({ HTMLAttributes }) { return ['div', mergeAttributes(HTMLAttributes, { 'data-type': 'link-preview' })]; },
  addNodeView() { return ReactNodeViewRenderer(LinkPreviewView); },
});

/** AUDENIQ 블록 — id와 짧은 이름만 저장, 값은 열 때마다 원천에서 읽는다 */
export const EntityEmbed = Node.create({
  name: 'entityEmbed',
  ...atomBlock,
  addAttributes() { return { kind: str('kind'), id: str('id'), view: str('view'), label: str('label') }; },
  parseHTML() { return [{ tag: 'div[data-type="entity-embed"]' }]; },
  renderHTML({ HTMLAttributes }) { return ['div', mergeAttributes(HTMLAttributes, { 'data-type': 'entity-embed' })]; },
  addNodeView() { return ReactNodeViewRenderer(EmbedView); },
});

/** @사람 · @문서 */
export const Mention = Node.create({
  name: 'mention',
  group: 'inline',
  inline: true,
  atom: true,
  selectable: false,
  addAttributes() { return { kind: str('kind'), id: str('id'), label: str('label') }; },
  parseHTML() { return [{ tag: 'span[data-type="mention"]' }]; },
  renderHTML({ node, HTMLAttributes }) {
    const kind = String(node.attrs.kind ?? 'user');
    const attrs = mergeAttributes(HTMLAttributes, { 'data-type': 'mention', class: `adm-doc-mention is-${kind}` });
    const text = `@${String(node.attrs.label ?? '')}`;
    // 문서 멘션은 앱 안 주소로만 (id는 서버가 uuid로 검증)
    return kind === 'doc' ? ['a', { ...attrs, href: `/admin/docs/${encodeURIComponent(String(node.attrs.id ?? ''))}` }, text] : ['span', attrs, text];
  },
  renderText({ node }) { return `@${String(node.attrs.label ?? '')}`; },
});

// ---------- 추천 창 ('/' 명령, '@' 멘션) ----------
export interface SuggestState<I> { kind: 'slash' | 'mention'; items: I[]; query: string; rect: DOMRect | null; command: (item: I) => void; range: Range }
export interface SuggestBridge {
  /** 열림·갱신 (null = 닫힘) */
  set: (s: SuggestState<unknown> | null) => void;
  /** 위·아래·Enter·Esc — 처리했으면 true */
  key: (e: KeyboardEvent) => boolean;
}

function suggestRender<I>(kind: 'slash' | 'mention', bridge: SuggestBridge) {
  return () => {
    const push = (p: SuggestionProps<I, I>) =>
      bridge.set({ kind, items: p.items as unknown[], query: p.query, rect: p.clientRect?.() ?? null, command: p.command as (i: unknown) => void, range: p.range });
    return {
      onStart: push,
      onUpdate: push,
      onKeyDown: (p: SuggestionKeyDownProps) => bridge.key(p.event),
      onExit: () => bridge.set(null),
    };
  };
}

export function SlashCommands<I>(opts: { items: (q: string) => I[]; run: (editor: Editor, range: Range, item: I) => void; bridge: SuggestBridge }) {
  return Extension.create({
    name: 'slashCommands',
    addProseMirrorPlugins() {
      return [Suggestion<I, I>({
        editor: this.editor,
        pluginKey: new PluginKey('slashCommands'),
        char: '/',
        allowSpaces: false,
        // 코드 블록 안의 '/'는 글자 그대로
        allow: ({ state, range }) => !state.doc.resolve(range.from).parent.type.spec.code,
        items: ({ query }) => opts.items(query),
        command: ({ editor, range, props }) => opts.run(editor, range, props),
        render: suggestRender<I>('slash', opts.bridge),
      })];
    },
  });
}

export function MentionSuggest<I>(opts: { items: (q: string) => Promise<I[]> | I[]; run: (editor: Editor, range: Range, item: I) => void; bridge: SuggestBridge }) {
  return Extension.create({
    name: 'mentionSuggest',
    addProseMirrorPlugins() {
      return [Suggestion<I, I>({
        editor: this.editor,
        pluginKey: new PluginKey('mentionSuggest'),
        char: '@',
        allowSpaces: false,
        items: ({ query }) => opts.items(query),
        command: ({ editor, range, props }) => opts.run(editor, range, props),
        render: suggestRender<I>('mention', opts.bridge),
      })];
    },
  });
}
