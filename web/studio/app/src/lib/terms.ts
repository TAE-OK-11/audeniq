// AUDENIQ 음원 배급 서비스 이용약관 — 원문(content/terms-ko.txt)을 화면용 블록으로 나눈다.
// 이름·판(version)은 lib/legal (원문을 고치면 그곳의 version도 올린다).
// 원문은 문서마다 따로 받는다 — 셋을 한 조각으로 묶으면 정산 약관 하나를 보려 해도 120KB를 다 받았다.
import { LEGAL_META, type LegalKey } from './legal';

export { TERMS_TITLE, TERMS_VERSION, type LegalKey } from './legal';

export type TermsBlock =
  | { kind: 'chapter'; id: string; text: string }
  | { kind: 'article'; id: string; text: string }
  | { kind: 'para'; text: string }
  | { kind: 'list'; items: string[] };

export interface ParsedTerms { meta: string[]; blocks: TermsBlock[] }

export function parseTerms(text: string): ParsedTerms {
  const lines = text.split('\n').map(l => l.trim()).filter(Boolean);
  const meta: string[] = [];
  const blocks: TermsBlock[] = [];
  let chapter = 0;
  for (const line of lines.slice(1)) {
    if (/^(시행일|최종 개정일|문서 버전|문서 상태|운영사|적용 서비스):/.test(line) && !blocks.length) { meta.push(line); continue; }
    if (/^영문명:/.test(line) && !blocks.length) continue;
    if (/^제\d+장\s/.test(line) || line === '부칙' || line === '사업자 정보') {
      chapter += 1;
      blocks.push({ kind: 'chapter', id: `c${chapter}`, text: line });
      continue;
    }
    const art = /^제(\d+)조\(/.exec(line);
    if (art) { blocks.push({ kind: 'article', id: `c${chapter}-a${art[1]}`, text: line }); continue; }
    const item = /^\d+\.\s+(.*)$/.exec(line);
    if (item) {
      const last = blocks.at(-1);
      if (last?.kind === 'list') last.items.push(item[1]);
      else blocks.push({ kind: 'list', items: [item[1]] });
      continue;
    }
    blocks.push({ kind: 'para', text: line });
  }
  return { meta, blocks };
}

/** 앱에서 볼 수 있는 약관 — /terms/<키> (이름·판·주소). 원문은 loadLegalText로 */
export const LEGAL_DOCS = LEGAL_META;

const LOADERS: Record<LegalKey, () => Promise<{ default: string }>> = {
  service: () => import('../content/terms-ko.txt?raw'),
  settlement: () => import('../content/settlement-terms-ko.txt?raw'),
  privacy: () => import('../content/privacy-ko.txt?raw'),
};
const loaded = new Map<LegalKey, string>();
const pending = new Map<LegalKey, Promise<string>>();

/** 이미 받은 원문 (없으면 undefined) — 다시 열 때 자리표시 없이 바로 그린다 */
export const legalTextNow = (doc: LegalKey) => loaded.get(doc);

/** 원문을 받는다 (같은 문서는 한 번만) */
export function loadLegalText(doc: LegalKey): Promise<string> {
  const hit = loaded.get(doc);
  if (hit !== undefined) return Promise.resolve(hit);
  let p = pending.get(doc);
  if (!p) {
    p = LOADERS[doc]().then(m => { loaded.set(doc, m.default); return m.default; }).finally(() => pending.delete(doc));
    pending.set(doc, p);
  }
  return p;
}
