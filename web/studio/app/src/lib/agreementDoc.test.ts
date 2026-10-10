import { expect, it } from 'vitest';
import { parseAgreement } from './agreementDoc';

const BODY = [
  'AUDENIQ 음원 배급 계약서', '',
  '서식 AUD-DIST 2.0 · 적용 약관 AUDENIQ 음원 배급 서비스 이용약관 (AUD-TERMS 2026.10)',
  '신청서 번호 AUD-20260920-ABCDEF', '',
  '주식회사 AUDENIQ(이하 “회사”)와 아래 이용자는 다음과 같이 계약합니다.', '',
  '제1조 (계약 당사자)', '회사: 주식회사 AUDENIQ', '이용자: 서린 (개인) · 아티스트 서린', '',
  '제6조 (배급수수료)', '배급수수료: 회사가 받은 수익 중 회사 8% / 이용자 92%', '',
  '제9조 (배급 권한의 부여)', '이용자는 약관 제11조부터 제13조까지에서 정한 범위의 권한을 회사에 부여합니다.', '',
  '별첨 1 · 트랙 목록', '1. 첫 번째 싱글 (ISRC KRA262600001)', '',
  '별첨 2 · 서명 시 확인 항목', '□ (필수) 권리를 보유합니다.', '□ (해당 시) 정산금 수령 권한이 있습니다.',
].join('\n');

it('계약서 본문을 머리·조·별첨으로 나눈다', () => {
  const p = parseAgreement(BODY);
  expect(p.meta).toEqual([['서식', 'AUD-DIST 2.0'], ['적용 약관', 'AUDENIQ 음원 배급 서비스 이용약관 (AUD-TERMS 2026.10)'], ['신청서 번호', 'AUD-20260920-ABCDEF']]);
  expect(p.intro).toHaveLength(1);
  expect(p.articles.map(a => a.no)).toEqual(['1', '6', '9']);
  expect(p.articles[0].blocks).toEqual([{ kind: 'kv', rows: [['회사', '주식회사 AUDENIQ'], ['이용자', '서린 (개인) · 아티스트 서린']] }]);
  expect(p.articles[2].blocks[0].kind).toBe('p');
  expect(p.annexes[0].blocks).toEqual([{ kind: 'list', items: ['첫 번째 싱글 (ISRC KRA262600001)'] }]);
  expect(p.annexes[1].blocks).toEqual([{ kind: 'check', items: [{ text: '권리를 보유합니다.', required: true }, { text: '정산금 수령 권한이 있습니다.', required: false }] }]);
});

it('예전(1.0) 짧은 본문은 문단으로만 남긴다', () => {
  const p = parseAgreement('AUDENIQ 디지털 음원 배급 신청·계약서\n\n발매: 첫 싱글\n신청인은 동의합니다.');
  expect(p.articles).toHaveLength(0);
  expect(p.intro).toEqual(['발매: 첫 싱글', '신청인은 동의합니다.']);
});
