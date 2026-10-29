// AUDENIQ 음원 배급 서비스 이용약관 — 원문(content/terms-ko.txt)을 화면용 블록으로 나눈다.
// 이름·판(version)은 lib/legal (원문을 고치면 그곳의 version도 올린다).
import raw from '../content/terms-ko.txt?raw';
import settlementRaw from '../content/settlement-terms-ko.txt?raw';
import privacyRaw from '../content/privacy-ko.txt?raw';
import { LEGAL_META, type LegalKey } from './legal';

export { TERMS_TITLE, TERMS_VERSION, type LegalKey } from './legal';
export const TERMS_TEXT = raw;

export type TermsBlock =
  | { kind: 'chapter'; id: string; text: string }
  | { kind: 'article'; id: string; text: string }
  | { kind: 'para'; text: string }
  | { kind: 'list'; items: string[] };

export interface ParsedTerms { meta: string[]; blocks: TermsBlock[] }

export function parseTerms(text: string = raw): ParsedTerms {
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

/** 앱에서 볼 수 있는 약관 — /terms/<키> (이름·판·주소 + 원문) */
const TEXT: Record<LegalKey, string> = { service: raw, settlement: settlementRaw, privacy: privacyRaw };
export const LEGAL_DOCS = Object.fromEntries(
  (Object.keys(LEGAL_META) as LegalKey[]).map(k => [k, { ...LEGAL_META[k], text: TEXT[k] }]),
) as Record<LegalKey, (typeof LEGAL_META)[LegalKey] & { text: string }>;
