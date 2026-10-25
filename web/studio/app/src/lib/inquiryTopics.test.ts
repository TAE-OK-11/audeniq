import { describe, expect, it } from 'vitest';
import { TOPICS, composeInquiry, findTopic, missingField, releaseFits } from './inquiryTopics';

describe('inquiry topics', () => {
  it('account and payout topics never ask for a release', () => {
    for (const t of TOPICS['계정·기타'].filter(t => t.key !== 'other')) expect(t.release).toBe('none');
    expect(findTopic('정산·지급', 'payout').release).toBe('none');
    expect(findTopic('본인인증·실명인증', undefined).release).toBe('none');
  });

  it('edits and cancellations need a release', () => {
    const edit = findTopic('수정·테이크다운', 'edit');
    expect(missingField(edit, { field: '제목', to: '여름' }, '', '')).toBe('관련 발매');
    expect(missingField(edit, { field: '제목' }, '', 'r1')).toBe('바꿀 값');
    expect(missingField(edit, { field: '제목', to: '여름' }, '', 'r1')).toBeNull();
  });

  it('asks for the text when nothing else was filled', () => {
    const t = findTopic('계약·권리', 'contract');
    expect(missingField(t, {}, '', '')).toBe('내용');
    expect(missingField(t, {}, '계약서를 다시 받고 싶어요', '')).toBeNull();
  });

  it('checks links', () => {
    const t = findTopic('계약·권리', 'dispute');
    expect(missingField(t, { url: 'youtube.com/x' }, '설명', '')).toBe('문제의 링크');
    expect(missingField(t, { url: 'https://youtube.com/x' }, '설명', '')).toBeNull();
  });

  it('filters releases by topic', () => {
    expect(releaseFits('live', 'live')).toBe(true);
    expect(releaseFits('live', 'review')).toBe(false);
    expect(releaseFits('before_live', 'review')).toBe(true);
    expect(releaseFits('before_live', 'live')).toBe(false);
    expect(releaseFits('submitted', 'draft')).toBe(false);
  });

  it('puts the topic in the subject and the fields on top of the body', () => {
    const t = findTopic('수정·테이크다운', 'edit');
    const c = composeInquiry(t, { field: '제목', from: 'Paper Moon', to: 'Paper Moon (Remaster)' }, '타이틀곡만 바꿔 주세요.', 'Paper Moon', '');
    expect(c.subject).toBe('[발매 정보 수정] Paper Moon');
    expect(c.body).toBe('고칠 항목: 제목\n지금 값: Paper Moon\n바꿀 값: Paper Moon (Remaster)\n\n타이틀곡만 바꿔 주세요.');
    expect(composeInquiry(findTopic('계정·기타', 'leave'), {}, '그만둘게요', '', '').subject).toBe('[탈퇴]');
  });
});
