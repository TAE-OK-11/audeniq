// AUDENIQ 약관 — 원문(content/*.txt)을 화면용 블록으로 나눈다.
// 원문을 고치면 해당 약관의 version도 올린다 (계약서·동의 기록이 어느 판에 동의했는지 남긴다).
import raw from '../content/terms-ko.txt?raw';
import settlementRaw from '../content/settlement-terms-ko.txt?raw';

export const TERMS_TITLE = 'AUDENIQ 음원 배급 서비스 이용약관';
export const TERMS_VERSION = 'AUD-TERMS 2026.10';
export const TERMS_TEXT = raw;

export const SETTLEMENT_TERMS_TITLE = 'AUDENIQ 정산금 지급 및 계좌 확인 약관';
export const SETTLEMENT_TERMS_VERSION = 'AUD-SETTLE 2.0';

/** 약관 목록 — /terms 화면의 문서 고르기와 동의 화면의 '약관 보기' 링크가 같이 쓴다 */
export interface TermsDoc { key: 'service' | 'settlement'; path: string; title: string; short: string; version: string; text: string }
export const TERMS_DOCS: readonly TermsDoc[] = [
  { key: 'service', path: '/terms', title: TERMS_TITLE, short: '서비스 이용약관', version: TERMS_VERSION, text: raw },
  { key: 'settlement', path: '/terms/settlement', title: SETTLEMENT_TERMS_TITLE, short: '정산금 지급 및 계좌 확인 약관', version: SETTLEMENT_TERMS_VERSION, text: settlementRaw },
];
export const termsDoc = (key: TermsDoc['key']): TermsDoc => TERMS_DOCS.find(d => d.key === key) ?? TERMS_DOCS[0];

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
    if (/^(시행일|최종 개정일|영문명|운영사|문서 버전|적용 서비스):/.test(line) && !blocks.length) { meta.push(line); continue; }
    if (/^제\d+장\s/.test(line) || ['부칙', '사업자 정보', '문의처', '약관 동의'].includes(line)) {
      chapter += 1;
      blocks.push({ kind: 'chapter', id: `c${chapter}`, text: line });
      continue;
    }
    const art = /^제(\d+)조\s?\(/.exec(line);
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
