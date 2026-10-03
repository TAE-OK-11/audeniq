import { expect, it } from 'vitest';
import { parseTerms } from './terms';

it('약관 원문을 장·조·목록으로 나눈다', () => {
  const { meta, blocks } = parseTerms();
  expect(meta[0]).toMatch(/^시행일:/);
  const articles = blocks.filter(b => b.kind === 'article');
  expect(articles).toHaveLength(103); // 본문 102조 + 부칙 제1조
  expect(articles[0]).toMatchObject({ text: '제1조(목적)' });
  expect(blocks.filter(b => b.kind === 'chapter').map(b => b.kind === 'chapter' && b.text)).toContain('제15장 정산');
  const defs = blocks.find(b => b.kind === 'list');
  expect(defs && defs.kind === 'list' && defs.items[0]).toMatch(/^“이용자”란/);
  // 장마다 조 번호가 겹치지 않게 id를 만든다 (부칙 제1조)
  const ids = articles.map(a => a.kind === 'article' && a.id);
  expect(new Set(ids).size).toBe(ids.length);
});
