// 문서 화면 공용 값 — 목록·편집기·노드 보기가 함께 쓴다.
// 편집기(TipTap, 500KB 넘음)와 따로 두어 문서 목록은 편집기 없이 열린다.
import { createContext } from 'react';
import type { People } from '../ui';
import type { EmbedStore } from './embeds';

export interface DocsCtx {
  embeds: EmbedStore;
  people: People;
  editable: boolean;
  inlineUrl: (attachmentId: string) => string;
  fileUrl: (attachmentId: string) => string;
}
export const DocsContext = createContext<DocsCtx | null>(null);
