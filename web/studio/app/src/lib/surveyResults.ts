// 설문 결과 집계·CSV — 화면(admin/pages/SurveyResults)과 테스트가 같이 쓴다.
import type { SurveyQuestion, SurveyResponseRow, SurveyResults } from '../api/content';

export interface OptionCount { label: string; count: number; share: number }
export interface QuestionTally { key: string; title: string; type: 'single' | 'multi'; answered: number; options: OptionCount[]; others: string[] }
export interface TextAnswer { id: string; created_at: string; text: string }

const picked = (v: string | string[] | undefined): string[] =>
  Array.isArray(v) ? v.filter(x => typeof x === 'string' && x !== '') : typeof v === 'string' && v !== '' ? [v] : [];

/** 문항별 선택 수 — 비율은 그 문항에 답한 사람 기준(여러 개 선택 문항은 합이 100%를 넘을 수 있다) */
export function tallyQuestion(q: SurveyQuestion, rows: SurveyResponseRow[], value = (r: SurveyResponseRow) => r.answers[q.key]): QuestionTally {
  const counts = q.options.map(() => 0);
  const others: string[] = [];
  let answered = 0;
  for (const r of rows) {
    const chosen = picked(value(r));
    if (!chosen.length) continue;
    answered++;
    for (const c of chosen) {
      const i = Number(c);
      if (Number.isInteger(i) && i >= 0 && i < counts.length) counts[i]++;
    }
    const other = r.answers[`${q.key}_other`];
    if (typeof other === 'string' && other.trim()) others.push(other.trim());
  }
  return {
    key: q.key, title: q.title, type: q.type, answered, others,
    options: q.options.map((label, i) => ({ label, count: counts[i], share: answered ? counts[i] / answered : 0 })),
  };
}

export function tallyAll(data: SurveyResults, rows = data.items): QuestionTally[] {
  return data.questions.map(q => tallyQuestion(q, rows));
}

export function tallyBeta(data: SurveyResults, rows = data.items): QuestionTally {
  return tallyQuestion({ key: 'beta', ...data.beta }, rows, r => r.beta);
}

export function textAnswers(rows: SurveyResponseRow[], key: string): TextAnswer[] {
  return rows.flatMap(r => {
    const v = r.answers[key];
    return typeof v === 'string' && v.trim() ? [{ id: r.id, created_at: r.created_at, text: v.trim() }] : [];
  });
}

/** 기간 필터 — 지금 기준 days일 안의 응답 */
export function withinDays(rows: SurveyResponseRow[], days: number | null, now: string): SurveyResponseRow[] {
  if (days == null) return rows;
  const from = Date.parse(now) - days * 86_400_000;
  return rows.filter(r => Date.parse(r.created_at) >= from);
}

const cell = (v: string) => (/[",\n\r]/.test(v) ? `"${v.replace(/"/g, '""')}"` : v);
const label = (q: { options: string[] }, v: string | string[] | undefined) =>
  picked(v).map(c => q.options[Number(c)] ?? c).join(' / ');

/** 엑셀에서 한글이 깨지지 않도록 BOM을 붙인 CSV (응답 1건 = 1줄) */
export function toCsv(data: SurveyResults, rows = data.items): string {
  const head = ['응답 ID', '제출 시각', ...data.questions.flatMap(q => [q.title, ...(q.options.includes('기타') ? [`${q.title} (기타)`] : [])]),
    ...data.text.map(t => t.title), data.beta.title, '연락처'];
  const lines = rows.map(r => [
    r.id, r.created_at,
    ...data.questions.flatMap(q => {
      const other = r.answers[`${q.key}_other`];
      return [label(q, r.answers[q.key]), ...(q.options.includes('기타') ? [typeof other === 'string' ? other : ''] : [])];
    }),
    ...data.text.map(t => { const v = r.answers[t.key]; return typeof v === 'string' ? v : ''; }),
    label(data.beta, r.beta), r.contact ?? '',
  ]);
  return '﻿' + [head, ...lines].map(l => l.map(v => cell(String(v))).join(',')).join('\r\n') + '\r\n';
}
