import { describe, expect, it } from 'vitest';
import type { SurveyResults } from '../api/content';
import { tallyAll, tallyBeta, textAnswers, toCsv, withinDays } from './surveyResults';

const data: SurveyResults = {
  now: '2026-10-08T00:00:00.000Z',
  total: 3,
  questions: [
    { key: 'q1', title: '활동 형태', type: 'multi', options: ['솔로', '밴드', '기타'] },
    { key: 'q2', title: '발매 경험', type: 'single', options: ['있다', '없다'] },
  ],
  text: [{ key: 'q14', title: '바라는 점' }],
  beta: { title: '베타', type: 'single', options: ['적극', '조건 확인', '소식만', '없음'] },
  items: [
    { id: 'a', version: 4, created_at: '2026-10-07T00:00:00.000Z', answers: { q1: ['0', '2'], q1_other: '세션', q2: '0', q14: '정산이 "빨랐으면", 해요' }, beta: '0', contact: '@a', contact_consent: true },
    { id: 'b', version: 4, created_at: '2026-10-05T00:00:00.000Z', answers: { q1: ['0'], q2: '1', q14: '' }, beta: '3', contact: null, contact_consent: false },
    { id: 'c', version: 4, created_at: '2026-08-01T00:00:00.000Z', answers: { q1: [], q2: '' }, beta: '', contact: null, contact_consent: false },
  ],
};

describe('survey results', () => {
  it('counts each option against people who answered the question', () => {
    const [q1, q2] = tallyAll(data);
    expect(q1.answered).toBe(2);
    expect(q1.options.map(o => o.count)).toEqual([2, 0, 1]);
    expect(q1.options[0].share).toBe(1);
    expect(q1.others).toEqual(['세션']);
    expect(q2.options.map(o => o.count)).toEqual([1, 1]);
    expect(tallyBeta(data).options.map(o => o.count)).toEqual([1, 0, 0, 1]);
  });

  it('filters by period and lists free text', () => {
    expect(withinDays(data.items, 7, data.now).map(r => r.id)).toEqual(['a', 'b']);
    expect(withinDays(data.items, null, data.now)).toHaveLength(3);
    expect(textAnswers(data.items, 'q14').map(t => t.id)).toEqual(['a']);
  });

  it('exports labelled CSV with a BOM and quoted cells', () => {
    const csv = toCsv(data);
    expect(csv.startsWith('﻿응답 ID,제출 시각,활동 형태,활동 형태 (기타),발매 경험,바라는 점,베타,연락처')).toBe(true);
    expect(csv).toContain('a,2026-10-07T00:00:00.000Z,솔로 / 기타,세션,있다,"정산이 ""빨랐으면"", 해요",적극,@a');
  });
});
