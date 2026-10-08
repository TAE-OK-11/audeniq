import { expect, it } from 'vitest';
import { parseTerms, termsDoc } from './terms';

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

it('정산금 지급 및 계좌 확인 약관도 같은 방식으로 나눈다', () => {
  const doc = termsDoc('settlement');
  expect(doc.path).toBe('/terms/settlement');
  const { meta, blocks } = parseTerms(doc.text);
  expect(meta).toContain('문서 버전: 2.0 (통합 최종 검토 초안)');
  expect(meta.some(m => m.startsWith('시행일:'))).toBe(true);
  const articles = blocks.filter(b => b.kind === 'article');
  expect(articles).toHaveLength(34); // 본문 31조 + 부칙 3조
  expect(articles[8]).toMatchObject({ text: '제9조(출금 수수료)' });
  const chapters = blocks.filter(b => b.kind === 'chapter').map(b => b.kind === 'chapter' && b.text);
  expect(chapters).toEqual(['제1장 총칙', '제2장 정산금의 산정 및 관리', '제3장 정산금 출금 및 지급', '제4장 금융계좌 등록 및 인증',
    '제5장 세금 및 외화 지급', '제6장 부정 정산 및 지급 보류', '제7장 오류, 이의신청 및 계약 종료', '제8장 개인정보 및 기타 사항', '부칙', '문의처', '약관 동의']);
  const ids = articles.map(a => a.kind === 'article' && a.id);
  expect(new Set(ids).size).toBe(ids.length);
  expect(doc.text).toContain('2027년 12월 31일 23시 59분 59초');
});
