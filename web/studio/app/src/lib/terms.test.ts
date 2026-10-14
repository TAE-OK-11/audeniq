import { expect, it } from 'vitest';
import { LEGAL_DOCS, parseTerms } from './terms';

it('약관 원문을 장·조·목록으로 나눈다', () => {
  const { meta, blocks } = parseTerms();
  expect(meta[0]).toMatch(/^시행일:/);
  const articles = blocks.filter(b => b.kind === 'article');
  expect(articles).toHaveLength(107); // 본문 106조 + 부칙 제1조
  expect(articles[0]).toMatchObject({ text: '제1조(목적)' });
  expect(blocks.filter(b => b.kind === 'chapter').map(b => b.kind === 'chapter' && b.text)).toContain('제15장 정산');
  const defs = blocks.find(b => b.kind === 'list');
  expect(defs && defs.kind === 'list' && defs.items[0]).toMatch(/^“이용자”란/);
  // 장마다 조 번호가 겹치지 않게 id를 만든다 (부칙 제1조)
  const ids = articles.map(a => a.kind === 'article' && a.id);
  expect(new Set(ids).size).toBe(ids.length);
});

it('정산 약관도 장·조로 나누고 머리 정보는 따로 모은다', () => {
  const { meta, blocks } = parseTerms(LEGAL_DOCS.settlement.text);
  expect(meta.some(m => m.startsWith('시행일:'))).toBe(true);
  expect(blocks[0]).toMatchObject({ kind: 'chapter', text: '제1장 총칙' });
  const titles = blocks.filter(b => b.kind === 'article').map(b => b.kind === 'article' && b.text);
  expect(titles).toContain('제29조(개인정보의 보호)');
  expect(titles).toContain('제14조(금융계좌 확인)');
});

it('개인정보 처리방침을 독립 문서로 제공하고 초안 상태를 표시한다', () => {
  const d = LEGAL_DOCS.privacy;
  expect(d.path).toBe('/privacy');
  const { meta, blocks } = parseTerms(d.text);
  expect(meta).toContain('문서 상태: 정식 시행 전 검토본');
  expect(blocks[0]).toMatchObject({ kind: 'chapter', text: '제1장 처리 원칙 및 항목' });
  const articles = blocks.filter(b => b.kind === 'article');
  expect(articles).toHaveLength(19);
  const ids = articles.map(a => a.kind === 'article' && a.id);
  expect(new Set(ids).size).toBe(ids.length);
  expect(d.text).toContain('제9조(개인정보의 국외 이전)');
  expect(d.text).toContain('제10조(정보주체의 권리와 행사방법)');
});

