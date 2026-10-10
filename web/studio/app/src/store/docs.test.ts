import { describe, expect, it } from 'vitest';
import { pendingProofs, proofPath, type DocRecord } from './docs';

const doc = (id: string, p: Partial<DocRecord>): DocRecord => ({
  id, kind: 'rights', title: id, releaseId: 'r1', releaseTitle: 'Aude2', version: '1.0', created: '2026-10-10 10:00',
  content: '', fileName: '', checked: false, checkedAt: '', consentHistory: [], reviewHistory: [],
  reviewStatus: 'awaiting_documents', reviewNote: '', signerName: '', localSignatureData: '', localSignatureAt: '', ...p,
});

describe('요청 서류', () => {
  it('발매의 제출 대기·보완 필요 권리 서류만 오래된 요청부터 고른다', () => {
    const list = [
      doc('later', { created: '2026-10-10 12:00', reviewStatus: 'needs' }),
      doc('first', { created: '2026-10-10 09:00' }),
      doc('submitted', { reviewStatus: 'review' }),
      doc('approved', { reviewStatus: 'approved' }),
      doc('other-release', { releaseId: 'r2' }),
      doc('agreement', { kind: 'agreements' }),
    ];
    expect(pendingProofs(list, 'r1').map(d => d.id)).toEqual(['first', 'later']);
  });

  it('요청 서류의 첨부·제출 화면으로 바로 간다', () => {
    expect(proofPath(doc('d 1', {}))).toBe('/rights?doc=d%201');
    expect(proofPath()).toBe('/rights');
  });
});
