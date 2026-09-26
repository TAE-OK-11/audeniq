// 설문 규칙 — 기존 public/app.js의 동작을 상태 기반 순수 함수로 옮겼다 (문구·조건 동일).
import { EXCLUSIVE, EXPERIENCE_BRANCH, FIRST, LAST, MULTI, STEPS, type Question, type Step } from './survey';

export interface SurveyState {
  /** 문항별 선택한 값 (단일 선택은 0~1개) */
  choices: Record<string, string[]>;
  /** 문항별 '기타' 직접 입력 */
  others: Record<string, string>;
  provider: string;
  comment: string;
  contact: string;
  betaConsent: boolean;
  surveyConsent: boolean;
}

export const emptyState = (): SurveyState => ({
  choices: {}, others: {}, provider: '', comment: '', contact: '', betaConsent: false, surveyConsent: false,
});

const QUESTIONS = new Map(STEPS.flatMap(s => s.questions).map(q => [q.key, q]));
export const question = (key: string) => QUESTIONS.get(key);

export const selected = (st: SurveyState, key: string) => st.choices[key] ?? [];
const first = (st: SurveyState, key: string) => selected(st, key)[0] ?? '';

/** 배급 경험이 없는 응답자 (발매 경험 없음 또는 '배급 서비스 이용 경험 없음') */
export function noDistributorExperience(st: SurveyState): boolean {
  return ['2', '3', '4'].includes(first(st, 'q2')) || selected(st, 'q4').includes('9');
}

/** '기타'(마지막 선택지)를 골랐는지 — 직접 입력칸을 보여 준다 */
export function otherOpen(st: SurveyState, q: Question): boolean {
  return !!q.other && !!q.options && selected(st, q.key).includes(String(q.options.length - 1));
}

/** 배급 경험이 없으면 경험 문항(q5·q6·q12)을 비운다 */
export function withBranch(st: SurveyState): SurveyState {
  if (!noDistributorExperience(st)) return st;
  const choices = { ...st.choices };
  const others = { ...st.others };
  for (const key of EXPERIENCE_BRANCH) {
    delete choices[key];
    delete others[key];
  }
  return { ...st, choices, others };
}

/** 선택지를 누른 결과 (배타 선택지, 최대 개수, 기타 입력칸, q2→q3·q4 연동, 경험 분기) */
export function applyChoice(st: SurveyState, key: string, value: string, checked: boolean): { state: SurveyState; error: string | null } {
  const q = question(key);
  if (!q) return { state: st, error: null };
  let list: string[];
  let error: string | null = null;
  if (q.kind === 'multi') {
    list = checked ? [...selected(st, key).filter(v => v !== value), value] : selected(st, key).filter(v => v !== value);
    const exclusive = EXCLUSIVE[key];
    if (exclusive !== undefined && checked) list = value === exclusive ? [value] : list.filter(v => v !== exclusive);
    if (q.max && list.length > q.max) {
      list = list.filter(v => v !== value);
      error = `이 문항은 최대 ${q.max}개까지 선택할 수 있어요.`;
    } else {
      error = '';
    }
  } else {
    list = checked ? [value] : [];
  }
  const choices = { ...st.choices, [key]: list };
  const others = { ...st.others };
  if (q.other && !(q.options && list.includes(String(q.options.length - 1)))) others[key] = '';
  let next: SurveyState = { ...st, choices, others };

  if (key === 'q2') {
    const noRelease = ['2', '3', '4'].includes(list[0] ?? '');
    const freq = first(next, 'q3');
    if (noRelease && freq !== '6') next = { ...next, choices: { ...next.choices, q3: ['6'] } };
    else if (!noRelease && freq === '6') next = { ...next, choices: { ...next.choices, q3: [] } };
  }
  if (key === 'q4' && list.includes('9')) next = { ...next, provider: '' };
  if (key === 'q2' || key === 'q4') next = withBranch(next);
  return { state: next, error };
}

const CONTACT_RE = /^(?:[^\s@]+@[^\s@]+\.[^\s@]+|@[A-Za-z0-9._]{1,30})$/;

/** 단계별 검사 — 첫 번째 문제의 안내 문구, 없으면 '' */
export function validateStep(st: SurveyState, step: Step): string {
  const skip = noDistributorExperience(st);
  for (const q of step.questions) {
    const key = q.key;
    if (key === 'q14' || (skip && EXPERIENCE_BRANCH.includes(key))) continue;
    if (key === 'beta') {
      const v = first(st, 'beta');
      const c = st.contact.trim();
      if (c && (v === '3' || v === '')) return '연락처를 입력하려면 베타 참여 또는 출시 안내 항목을 선택해 주세요.';
      if (c && !CONTACT_RE.test(c)) return '이메일 주소 또는 @로 시작하는 Instagram 계정을 입력해 주세요.';
      if (c && !st.betaConsent) return '연락처를 입력했다면 연락처 수집·이용에 동의해 주세요.';
      if (!c && st.betaConsent) return '연락처 동의 체크를 해제하거나 연락처를 입력해 주세요.';
      if (!st.surveyConsent) return '설문 응답 수집·이용에 동의해 주세요.';
      continue;
    }
    const s = selected(st, key);
    if (key === 'q3' && ['2', '3', '4'].includes(first(st, 'q2')) && s[0] !== '6') return '발매 경험이 없다면 ‘아직 정식 음원을 발매한 경험이 없음’을 선택해 주세요.';
    if (key === 'q3' && ['0', '1'].includes(first(st, 'q2')) && s[0] === '6') return '발매 경험이 있다면 최근 12개월간 발매 여부에 맞춰 응답해 주세요.';
    if (q.required && !s.length) return `${key.toUpperCase()} 필수 문항에 응답해 주세요.`;
    if (key === 'q15' && s.includes('13') && s.length > 1) return '‘잘 모르겠다’는 다른 플랫폼과 함께 선택할 수 없어요.';
    if (q.max && s.length > q.max) return `${key.toUpperCase()}는 최대 ${q.max}개까지 선택할 수 있어요.`;
    if (otherOpen(st, q) && !(st.others[key] ?? '').trim()) return `${key.toUpperCase()}의 기타 내용을 적어 주세요.`;
  }
  return '';
}

export const validateAll = (st: SurveyState) => STEPS.map(step => validateStep(st, step)).find(Boolean) ?? '';

/** POST /api/responses 본문 (기존과 같은 모양) */
export function payload(st: SurveyState, submissionId: string, turnstileToken: string) {
  const answers: Record<string, string | string[]> = {};
  for (let i = FIRST; i <= LAST; i++) {
    const key = `q${i}`;
    answers[key] = MULTI.has(key) ? selected(st, key) : first(st, key);
    const q = question(key);
    if (q?.other) answers[`${key}_other`] = otherOpen(st, q) ? (st.others[key] ?? '').trim() : '';
  }
  answers.q4_provider = selected(st, 'q4').includes('9') ? '' : st.provider.trim();
  answers.q14 = st.comment.trim();
  return {
    submission_id: submissionId,
    answers,
    beta: first(st, 'beta'),
    contact: st.contact.trim(),
    beta_contact_consent: st.betaConsent,
    survey_consent: st.surveyConsent,
    turnstile_token: turnstileToken,
  };
}
