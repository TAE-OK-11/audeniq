import { expect, it } from 'vitest';
import { RIGHTS_FORM, rightsDocumentBody, rightsHashInput } from './rightsDocument';
import { sha256Hex } from './application';

const context = { releaseId: 'r1', title: '커버 싱글', artist: 'Artist', tracks: [{ title: 'Cover', isrc: 'KRABC2600001' }], territories: ['WORLD'], platforms: ['spotify'] };
const input = { documentNo: 'receipt', rightsHolder: '작곡가', signerName: '작곡가', signerRole: '권리자 본인', source: 'Original / Writer', scope: '작곡 지분 50% · 디지털 배급', period: '2026년 12월까지', conditions: '순수익의 30%를 분기마다 정산' };

it('발매 정보·당사자·허락 범위와 법적 필수 조항을 채운다', () => {
  const body = rightsDocumentBody('composition', context, input);
  expect(body).toContain('ISRC KRABC2600001');
  expect(body).toContain('지분 50%');
  expect(body).toContain('Spotify');
  expect(body).toContain(`서식 ${RIGHTS_FORM} · 문서번호 receipt`);
  expect(body).toContain('허락자(권리자): 작곡가');
  expect(body).toContain('이용자: Artist');
  for (const clause of ['제2조 (기간)', '제3조 (대가와 조건)', '제4조 (권리 보증)', '제5조 (철회)', '제6조 (전자문서와 전자서명)', '제7조 (사본과 보관)', '대한민국 법']) {
    expect(body).toContain(clause);
  }
  expect(body).toContain('「전자서명법」 제3조');
});

it('위임 확인서는 위임인·수임인, 대리인 서명은 권리자를 대신한다고 적는다', () => {
  const body = rightsDocumentBody('shared', context, { ...input, rightsHolder: '공동 작곡가', signerName: '대리인', signerRole: '권리자의 위임을 받은 대리인' });
  expect(body).toContain('위임인(권리자): 공동 작곡가');
  expect(body).toContain('수임인: Artist');
  expect(body).toContain('서명자: 대리인 (권리자의 위임을 받은 대리인, 권리자: 공동 작곡가)');
});

it('권리자·본문·서명이 바뀌면 확인 코드가 달라진다', async () => {
  const body = rightsDocumentBody('composition', context, input);
  const e = { document_no: 'receipt', form: RIGHTS_FORM, document_kind: 'composition', rights_holder: '작곡가', signer_name: '작곡가', signer_role: '권리자 본인', signature: 'PNG' };
  const hash = await sha256Hex(rightsHashInput('허락서', body, e));
  expect(await sha256Hex(rightsHashInput('허락서', body + '변경', e))).not.toBe(hash);
  expect(await sha256Hex(rightsHashInput('허락서', body, { ...e, signature: 'OTHER' }))).not.toBe(hash);
  expect(await sha256Hex(rightsHashInput('허락서', body, { ...e, rights_holder: '다른 권리자' }))).not.toBe(hash);
});
