import { expect, it } from 'vitest';
import { RIGHTS_FORM, rightsDocumentBody, rightsHashInput } from './rightsDocument';
import { sha256Hex } from './application';

it('발매 정보와 허락 범위를 채우고, 권리자·본문·서명 변경을 확인 코드에 반영한다', async () => {
  const context = { releaseId: 'r1', title: '커버 싱글', artist: 'Artist', tracks: [{ title: 'Cover', isrc: 'KRABC2600001' }], territories: ['WORLD'], platforms: ['spotify'] };
  const input = { rightsHolder: '작곡가', source: 'Original / Writer', scope: '작곡 지분 50% · 디지털 배급', period: '2026년 12월까지', conditions: '별도 계약의 대가 조건' };
  const body = rightsDocumentBody('composition', context, input);
  expect(body).toContain('ISRC KRABC2600001');
  expect(body).toContain('지분 50%');
  expect(body).toContain('Spotify');
  const e = { document_no: 'receipt', form: RIGHTS_FORM, document_kind: 'composition' as const, rights_holder: '작곡가', signer_name: '작곡가', signer_role: '권리자 본인', signature: 'PNG', consent: true };
  const hash = await sha256Hex(rightsHashInput('허락서', body, e));
  expect(await sha256Hex(rightsHashInput('허락서', body + '변경', e))).not.toBe(hash);
  expect(await sha256Hex(rightsHashInput('허락서', body, { ...e, signature: 'OTHER' }))).not.toBe(hash);
  expect(await sha256Hex(rightsHashInput('허락서', body, { ...e, rights_holder: '다른 권리자' }))).not.toBe(hash);
});
