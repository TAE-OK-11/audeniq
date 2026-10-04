// 문서 내용(Tiptap/ProseMirror JSON)을 서버가 받는 모양으로 맞춘다. 서버(crates/core/src/work/doc_model.rs)가
// 허용 노드·마크·속성만 받으므로, 편집기가 덧붙이는 값(링크 title, 인라인 노드의 marks, 기본값 null 등)은
// 저장 전에 덜어 낸다. 보안 판단은 서버가 하고, 여기서는 400을 피하기 위한 정리만 한다.
import type { DocNode } from '../types';

/** 노드별로 남길 속성 — 서버 허용 목록과 같다 */
const NODE_ATTRS: Record<string, string[]> = {
  doc: [], text: [], paragraph: [], blockquote: [], bulletList: [], listItem: [], taskList: [], tableRow: [],
  detailsContent: [], detailsSummary: [], horizontalRule: [], hardBreak: [], table: [],
  heading: ['level'], orderedList: ['start', 'type'], taskItem: ['checked'], codeBlock: ['language'],
  tableHeader: ['colspan', 'rowspan', 'colwidth'], tableCell: ['colspan', 'rowspan', 'colwidth'], details: ['open'],
  callout: ['tone'], docImage: ['attachmentId', 'alt', 'width'], fileAttachment: ['attachmentId', 'name', 'size'],
  linkPreview: ['url', 'title', 'description'], entityEmbed: ['kind', 'id', 'view', 'label'], mention: ['kind', 'id', 'label'],
};
const MARK_ATTRS: Record<string, string[]> = { bold: [], italic: [], strike: [], code: [], underline: [], link: ['href', 'target', 'rel'] };

export const SAFE_URL = /^(https?:\/\/[^\s/?#]+[^\s]*|mailto:[^\s@]+@[^\s]+)$/i;
export const isSafeUrl = (u: string) => u.length <= 2048 && SAFE_URL.test(u.trim());

function pick(attrs: Record<string, unknown> | undefined, keys: string[]): Record<string, unknown> | undefined {
  if (!attrs || keys.length === 0) return undefined;
  const out: Record<string, unknown> = {};
  for (const k of keys) {
    const v = attrs[k];
    if (v !== undefined && v !== null && v !== '') out[k] = v;
  }
  return Object.keys(out).length ? out : undefined;
}

/** 서버로 보낼 문서 — 모르는 노드는 버리지 않고 글자만 남긴 문단으로 바꾼다 */
export function normalizeDoc(node: DocNode): DocNode {
  const walk = (n: DocNode): DocNode | null => {
    if (n.type === 'text') {
      if (!n.text) return null;
      const marks = (n.marks ?? [])
        .filter(m => m.type in MARK_ATTRS && (m.type !== 'link' || isSafeUrl(String(m.attrs?.href ?? ''))))
        .map(m => {
          const attrs = pick(m.attrs, MARK_ATTRS[m.type]);
          return attrs ? { type: m.type, attrs } : { type: m.type };
        });
      return marks.length ? { type: 'text', text: n.text, marks } : { type: 'text', text: n.text };
    }
    const keys = NODE_ATTRS[n.type];
    if (!keys) {
      const text = plainText(n);
      return text ? { type: 'paragraph', content: [{ type: 'text', text }] } : null;
    }
    const out: DocNode = { type: n.type };
    const attrs = pick(n.attrs, keys);
    if (attrs) out.attrs = attrs;
    if (n.content) {
      const content = n.content.map(walk).filter((c): c is DocNode => c != null);
      if (content.length) out.content = content;
    }
    return out;
  };
  return walk(node) ?? { type: 'doc', content: [{ type: 'paragraph' }] };
}

/** 블록 사이를 줄 바꿈으로 이은 글자 (버전 비교·미리보기) */
export function plainText(node: DocNode): string {
  const lines: string[] = [];
  let line = '';
  const BLOCK = new Set(['paragraph', 'heading', 'codeBlock', 'listItem', 'taskItem', 'tableRow', 'detailsSummary', 'callout', 'blockquote']);
  const walk = (n: DocNode) => {
    if (n.type === 'text') { line += n.text ?? ''; return; }
    if (n.type === 'hardBreak') { lines.push(line); line = ''; return; }
    if (n.type === 'mention') { line += `@${String(n.attrs?.label ?? '')}`; return; }
    if (n.type === 'entityEmbed') { lines.push(`[${String(n.attrs?.kind ?? '')}] ${String(n.attrs?.label ?? n.attrs?.id ?? '')}`); return; }
    if (n.type === 'docImage') { lines.push(`[이미지] ${String(n.attrs?.alt ?? '')}`); return; }
    if (n.type === 'fileAttachment') { lines.push(`[파일] ${String(n.attrs?.name ?? '')}`); return; }
    if (n.type === 'linkPreview') { lines.push(`[링크] ${String(n.attrs?.title ?? n.attrs?.url ?? '')}`); return; }
    if (n.type === 'horizontalRule') { lines.push('---'); return; }
    for (const c of n.content ?? []) walk(c);
    if (n.type === 'tableCell' || n.type === 'tableHeader') line += ' | ';
    if (BLOCK.has(n.type) && line) { lines.push(line); line = ''; }
  };
  walk(node);
  if (line) lines.push(line);
  return lines.join('\n');
}

/** 문서가 넣은 AUDENIQ 블록 — 'kind:id[:view]' (임베드 일괄 조회 키) */
export function embedKeys(node: DocNode): string[] {
  const out = new Set<string>();
  const walk = (n: DocNode) => {
    if (n.type === 'entityEmbed' && n.attrs?.kind && n.attrs?.id) out.add(embedKey(String(n.attrs.kind), String(n.attrs.id), String(n.attrs.view ?? 'summary')));
    for (const c of n.content ?? []) walk(c);
  };
  walk(node);
  return [...out];
}

export const embedKey = (kind: string, id: string, view = 'summary') => (view && view !== 'summary' ? `${kind}:${id}:${view}` : `${kind}:${id}`);

export const EMPTY_DOC: DocNode = { type: 'doc', content: [{ type: 'paragraph' }] };
