// AUDENIQ 음원 배급 서비스 이용약관 — 원문(content/terms-ko.txt)을 화면용 블록으로 나눈다.
// 원문을 고치면 TERMS_VERSION도 올린다 (배급 계약서·동의 기록이 어느 판에 동의했는지 남긴다).
import raw from '../content/terms-ko.txt?raw';
import settlementRaw from '../content/settlement-terms-ko.txt?raw';

export const TERMS_TITLE = 'AUDENIQ 음원 배급 서비스 이용약관';
export const TERMS_VERSION = 'AUD-TERMS 2026.10';
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
    if (/^(시행일|최종 개정일|문서 버전|운영사|적용 서비스):/.test(line) && !blocks.length) { meta.push(line); continue; }
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

/** 앱에서 볼 수 있는 약관 — /terms/<키>. 원문을 고치면 version을 올린다. */
export type LegalKey = 'service' | 'settlement';
export const LEGAL_DOCS: Record<LegalKey, { title: string; short: string; version: string; text: string; path: string }> = {
  service: { title: TERMS_TITLE, short: '이용약관', version: TERMS_VERSION, text: raw, path: '/terms' },
  settlement: {
    title: 'AUDENIQ 정산금 지급 및 계좌 확인 약관', short: '정산 약관', version: 'AUD-SETTLE 2.0',
    text: settlementRaw, path: '/terms/settlement',
  },
};
