// 저작권 관리(곡·권리) — 화면이 함께 쓰는 이름표·상태 색·지분 검증.
// 서버 응답 모양은 api/publishing.ts, 여기는 계산과 글자만 (테스트: publishing.test.ts)
import type { Tone } from '../ui/tone';
import type {
  ApplicationStatus, ArStatus, ContractStatus, ContributorRole, RegistrationStatus, RoyaltyKind, RoyaltyStatus, Work, WorkContributor, WorkFix,
} from '../api/publishing';

/** 곡 등록(권리 관리) 진행 — 기획안 7.1
 *  색은 발매 상태와 같은 규칙(DESIGN_SYSTEM 규칙 5): 확인·심사가 진행 중이면 보라, 보완 주황, 완료 초록, 불가 빨강 */
export const REG_LABEL: Record<RegistrationStatus, string> = {
  none: '미신청',
  preparing: '신청 준비 중',
  internal_review: '내부 확인 중',
  needs_fix: '보완 필요',
  ready: '등록 준비 완료',
  submitted: '외부 제출 완료',
  external_review: '외부 확인 중',
  registered: '등록 완료',
  rejected: '등록 불가',
};

/** 상태를 한 문장으로 — 곡 상세 머리에 짧게 (기획안 7.2) */
export const REG_SENTENCE: Record<RegistrationStatus, string> = {
  none: '아직 저작권 관리를 신청하지 않았어요.',
  preparing: '신청서를 작성하고 있어요.',
  internal_review: '담당자가 권리 정보를 확인하고 있어요.',
  needs_fix: '보완할 정보·서류가 있어요. 고쳐서 내면 확인이 이어져요.',
  ready: '권리 정보 확인이 끝났어요. 관리 단체 등록을 준비하고 있어요.',
  submitted: '관리 단체에 등록을 신청했어요.',
  external_review: '관리 단체가 등록 내용을 확인하고 있어요.',
  registered: '저작권 등록이 끝났어요.',
  rejected: '이 곡은 등록할 수 없어요. 사유를 확인해 주세요.',
};

export const REG_TONE: Record<RegistrationStatus, Tone> = {
  none: 'gray', preparing: 'gray', internal_review: 'violet', needs_fix: 'amber', ready: 'violet',
  submitted: 'violet', external_review: 'violet', registered: 'green', rejected: 'red',
};

/** 진행 막대에 쓰는 단계 — 보완 필요·등록 불가는 단계가 아니라 그 자리의 막힘 */
export const REG_STEPS = ['신청', '내부 확인', '등록 준비', '외부 제출', '등록 완료'] as const;
export function regStep(s: RegistrationStatus): number {
  switch (s) {
    case 'none': return -1;
    case 'preparing': return 0;
    case 'internal_review': case 'needs_fix': return 1;
    case 'ready': return 2;
    case 'submitted': case 'external_review': case 'rejected': return 3;
    case 'registered': return 5;
  }
}

/** 담당자가 고르는 보완 항목 — 서버 fix_field_label(crates/core/src/publishing.rs)과 같은 키.
 *  곡 상세의 보완 카드는 항목마다 고칠 자리로 데려간다 (참여자 쪽은 참여자·지분, 곡 쪽은 곡 정보 고치기) */
export const FIX_FIELDS = {
  'contributors.split': '참여자·지분',
  'contributors.ipi': '참여자 IPI 번호',
  'contributors.society': '저작권 관리 단체',
  'work.info': '곡 정보',
  'work.prior': '기존 퍼블리셔',
} as const;
export type FixField = keyof typeof FIX_FIELDS;
export const isFixField = (f: string): f is FixField => f in FIX_FIELDS;

/** 담당자가 따로 남긴 의견 — 있을 때만 '검토팀' 말풍선으로 보인다. 예전 요청(note 없음)은 reason이 담당자의 글이다 */
export function fixNote(f: WorkFix): string | null {
  const text = f.note === undefined ? f.reason : f.note;
  return text?.trim() || null;
}

/** 신청서에 서명하며 확인하는 항목 — 서버 AGREEMENTS(crates/core/src/publishing.rs)와 같은 키.
 *  계약서의 확인 항목(CONTRACT_CONFIRMATIONS)과는 다르다: 신청서는 '신청 내용이 맞다', 계약서는 '계약을 맺는다' */
export const APPLICATION_AGREE = [
  ['accurate', '신청서에 적은 곡·참여자·지분 정보가 사실과 같아요.'],
  ['rights', '신청한 곡에 대해 적은 지분만큼 권리를 가지고 있어요.'],
  ['prior', '다른 퍼블리셔와 맺은 계약이 있으면 곡 정보에 알렸어요.'],
  ['share', '공동 참여자의 지분은 각자의 확인을 거쳐 반영된다는 것을 알아요.'],
] as const;

/** 저작권 관리 계약서에 서명하기 전 체크하는 항목 — 서버 CONTRACT_CONFIRMATIONS와 같다 (모두 필수) */
export const CONTRACT_CONFIRMATIONS = [
  ['read', '저작권 관리 계약서의 내용을 모두 읽고 이해했어요.'],
  ['rights', '계약한 곡에 대해 신고한 지분만큼 권리를 가지고 있어요.'],
  ['exclusive', '계약 기간 동안 같은 권리를 다른 퍼블리셔에게 맡기지 않아요.'],
] as const;

const TERRITORY_NAME: Record<string, string> = { KR: '대한민국', JP: '일본', US: '미국', WORLD: '전 세계(그 밖의 국가)' };
const RIGHT_NAME: Record<string, string> = { performance: '공연권', mechanical: '복제권', digital: '전송권', sync: '싱크 라이선스' };
export const territoryName = (c: string) => TERRITORY_NAME[c] ?? c;
export const rightName = (k: string) => RIGHT_NAME[k] ?? k;

/** 계약서 본문 — 서버 contract_body와 같은 문장 (체험 데이터용; 실서버는 승인할 때 서버가 만든다) */
export function contractBody(works: readonly (readonly [string, string])[], territories: readonly string[], rights: readonly string[], rate: number, months: number, renewals: readonly number[]): string {
  return [
    '제1조 (목적) 이 계약은 작가가 아래 곡의 저작권 관리를 주식회사 AUDENIQ(이하 ‘회사’)에 맡기고, 회사가 이를 관리하는 데 필요한 사항을 정한다.',
    `제2조 (대상 곡) ${works.map(([t, code]) => `‘${t}’(${code})`).join(', ')}. 작가는 각 곡에 신고한 지분만큼의 권리를 가진다.`,
    `제3조 (관리 범위) 회사는 ${territories.map(territoryName).join(', ')}에서 ${rights.map(rightName).join(', ')}을(를) 관리한다. 관리 단체 등록, 사용료 징수, 이용 허락 협의를 포함한다.`,
    `제4조 (수수료) 회사는 실제 징수한 사용료의 ${rate}%를 관리 수수료로 공제하고 나머지를 작가에게 지급한다. 예상 금액은 지급 대상이 아니다.`,
    `제5조 (기간) 계약 기간은 작가가 서명한 날부터 ${termLabel(months)}로 하며, 작가가 요청하면 ${renewals.map(termLabel).join(', ')} 단위로 연장한다.`,
    '제6조 (공동 저작) 공동 참여자의 지분은 각 참여자의 확인을 거쳐 반영한다. 확인되지 않은 지분은 확인될 때까지 사용료 지급을 보류할 수 있다.',
    '제7조 (종료) 어느 한쪽이 종료를 요청하면 관리 단체 절차에 따라 관리를 마친다. 종료 전 발생한 사용료는 종료 후에도 정산한다.',
    '제8조 (기존 계약) 작가는 다른 퍼블리셔와의 계약을 신고해야 하며, 충돌이 있으면 회사가 검토한 뒤 관리 범위를 조정할 수 있다.',
    '제9조 (전자계약) 이 계약은 전자문서로 작성하고 전자서명으로 체결한다. 체결일은 작가가 서명한 날이며, 서명·확인 항목·문서 확인 코드는 함께 보관한다.',
  ].join('\n');
}

/** 역할 이름 — '작사·작곡' */
export const roleTextOf = (roles: readonly ContributorRole[]) => roles.map(r => ROLE_LABEL[r]).join('·') || '역할 없음';

export const APP_LABEL: Record<ApplicationStatus, string> = {
  draft: '작성 중', submitted: '제출 완료', in_review: '심사 중', needs_fix: '보완 요청', approved: '승인', rejected: '반려', withdrawn: '철회',
};
export const APP_TONE: Record<ApplicationStatus, Tone> = {
  draft: 'gray', submitted: 'violet', in_review: 'violet', needs_fix: 'amber', approved: 'green', rejected: 'red', withdrawn: 'gray',
};

export const CONTRACT_LABEL: Record<ContractStatus, string> = {
  pending_sign: '계약서 서명 필요', active: '관리 중', expiring: '곧 만료', ended: '종료', terminating: '종료 요청됨',
};
export const CONTRACT_TONE: Record<ContractStatus, Tone> = {
  pending_sign: 'blue', active: 'green', expiring: 'amber', ended: 'gray', terminating: 'red',
};

export const ROLE_LABEL: Record<ContributorRole, string> = {
  lyricist: '작사', composer: '작곡', arranger: '편곡', other: '기타',
};

export const ROYALTY_KIND_LABEL: Record<RoyaltyKind, string> = {
  performance: '공연권', mechanical: '복제권', digital: '전송권', sync: '싱크', print: '출판', other: '기타',
};
export const ROYALTY_STATUS_LABEL: Record<RoyaltyStatus, string> = { estimated: '예상', confirmed: '확정', paid: '지급 완료' };
export const ROYALTY_STATUS_TONE: Record<RoyaltyStatus, Tone> = { estimated: 'gray', confirmed: 'blue', paid: 'green' };

export const AR_LABEL: Record<ArStatus, string> = {
  submitted: '제출 완료', in_review: '심사 중', needs_fix: '보완 요청', reviewed: '심사 완료', proposing: '제안 진행 중', closed: '종료',
};
export const AR_TONE: Record<ArStatus, Tone> = {
  submitted: 'violet', in_review: 'violet', needs_fix: 'amber', reviewed: 'blue', proposing: 'green', closed: 'gray',
};

export const WORK_TYPES = [
  ['original', '창작곡'], ['arrangement', '편곡'], ['translation', '번안·개사'], ['instrumental', '연주곡'], ['medley', '메들리'],
] as const;
export const LANGUAGES = [['ko', '한국어'], ['en', '영어'], ['ja', '일본어'], ['zh', '중국어'], ['es', '스페인어'], ['zxx', '가사 없음'], ['other', '기타']] as const;
export const SOCIETIES = ['KOMCA', '함께하는음악저작인협회', 'JASRAC', 'ASCAP', 'BMI', 'SESAC', 'PRS', 'GEMA', 'SACEM', '없음'] as const;

/** 저장 단위 소수점 둘째 자리까지 — 33.33 + 33.33 + 33.34 같은 나누기를 받는다 */
const round2 = (n: number) => Math.round(n * 100) / 100;
export const sumShares = (list: readonly WorkContributor[], key: 'share' | 'publisherShare' = 'share') =>
  round2(list.reduce((n, c) => n + (Number.isFinite(c[key]) ? c[key] : 0), 0));

export interface SplitIssue { level: 'error' | 'warn'; text: string; contributorId?: string }

/**
 * 지분 확인 — 임시 저장은 늘 되고, 이 결과의 error가 하나라도 있으면 저작권 관리 신청은 막는다 (기획안 5.3, 12)
 */
export function splitIssues(list: readonly WorkContributor[]): SplitIssue[] {
  const out: SplitIssue[] = [];
  if (!list.length) return [{ level: 'error', text: '참여자를 한 명 이상 추가해 주세요.' }];
  const total = sumShares(list);
  if (total !== 100) out.push({ level: 'error', text: total > 100 ? `창작 지분 합계가 ${total}%예요. 100%를 넘을 수 없어요.` : `창작 지분 합계가 ${total}%예요. 100%가 되도록 나눠 주세요.` });
  for (const c of list) {
    const who = c.name.trim() || '이름 없는 참여자';
    if (!c.name.trim()) out.push({ level: 'error', text: '이름이 비어 있는 참여자가 있어요.', contributorId: c.id });
    if (!c.roles.length) out.push({ level: 'error', text: `${who}의 역할(작사·작곡 등)을 골라 주세요.`, contributorId: c.id });
    if (c.share < 0 || c.share > 100 || !Number.isFinite(c.share)) out.push({ level: 'error', text: `${who}의 지분은 0~100% 사이로 적어 주세요.`, contributorId: c.id });
    if (c.publisherShare < 0 || c.publisherShare > c.share) out.push({ level: 'error', text: `${who}의 퍼블리셔 관리 지분은 창작 지분(${c.share}%)보다 클 수 없어요.`, contributorId: c.id });
    if (c.confirm === 'disputed') out.push({ level: 'error', text: `${who}이(가) 지분에 이의를 제기했어요. 지분을 다시 맞춰 주세요.`, contributorId: c.id });
    else if (c.confirm === 'pending') out.push({ level: 'warn', text: `${who}의 지분 확인을 기다리고 있어요.`, contributorId: c.id });
  }
  if (list.every(c => !c.roles.includes('composer'))) out.push({ level: 'warn', text: '작곡가가 없어요. 연주곡이라도 작곡가는 꼭 있어야 해요.' });
  return out;
}

export const splitReady = (list: readonly WorkContributor[]) => !splitIssues(list).some(i => i.level === 'error');

/** 남은 지분을 고르게 나눈 값 — '균등하게 나누기' 버튼. 마지막 사람이 나머지를 받아 합계가 정확히 100 */
export function evenSplit(n: number): number[] {
  if (n <= 0) return [];
  const each = Math.floor((100 / n) * 100) / 100;
  return Array.from({ length: n }, (_, i) => (i === n - 1 ? round2(100 - each * (n - 1)) : each));
}

/** 저작권 관리에 들어가 있는(신청 이후) 곡인지 */
export const isManaged = (w: Pick<Work, 'registration'>) => !['none', 'preparing', 'rejected'].includes(w.registration);

/** 라이브러리 필터 — 기획안 2.2 */
export const WORK_FILTERS = [
  ['all', '전체 곡'],
  ['mine', '내가 참여한 곡'],
  ['owned', '내가 관리하는 곡'],
  ['managed', '저작권 관리 중'],
  ['none', '저작권 관리 미신청'],
  ['review', '심사 중'],
  ['fix', '보완 필요'],
  ['unreleased', '미공개 곡'],
] as const;
export type WorkFilter = typeof WORK_FILTERS[number][0];

export function matchFilter(w: Work, f: WorkFilter): boolean {
  switch (f) {
    case 'all': return true;
    case 'mine': return w.contributors.some(c => c.isMe);
    case 'owned': return w.myRole === 'owner' || w.myRole === 'editor';
    case 'managed': return isManaged(w) && w.registration !== 'needs_fix';
    case 'none': return w.registration === 'none' || w.registration === 'preparing';
    case 'review': return w.registration === 'internal_review' || w.application?.status === 'in_review' || w.application?.status === 'submitted';
    case 'fix': return w.registration === 'needs_fix' || w.application?.status === 'needs_fix';
    case 'unreleased': return w.recordings.length === 0;
  }
}

/** 크레딧 글자('서린, 김하늘 / 박') → 이름 목록 — 발매 정보에서 곡을 만들 때 참여자를 미리 채운다 */
export function splitNames(s: string | null | undefined): string[] {
  return Array.from(new Set(String(s ?? '').split(/[,/·&]|\s+(?:and|feat\.?)\s+/i).map(x => x.trim()).filter(Boolean)));
}

/** 이름이 같거나 띄어쓰기·대소문자만 다른 곡 — '이미 있는 곡에 연결할까요?' (기획안 12 중복) */
export function similarWorks(title: string, works: readonly Work[], exceptId?: string): Work[] {
  const k = title.toLowerCase().replace(/[\s()[\]\-_.]/g, '');
  if (k.length < 2) return [];
  return works.filter(w => w.id !== exceptId && [w.title, ...w.altTitles].some(t => {
    const o = t.toLowerCase().replace(/[\s()[\]\-_.]/g, '');
    return o === k || (o.length >= 3 && (o.includes(k) || k.includes(o)));
  }));
}

/** 계약 기간 개월 → '6개월', '1년', '3년' */
export function termLabel(months: number): string {
  return months % 12 === 0 ? `${months / 12}년` : `${months}개월`;
}

/** 남은 날 — 만료 안내 */
export function daysLeft(end: string, now = new Date()): number {
  const d = new Date(`${end}T23:59:59+09:00`).getTime() - now.getTime();
  return Math.ceil(d / 86_400_000);
}

export const pct = (n: number) => `${Number.isInteger(n) ? n : n.toFixed(2).replace(/0$/, '')}%`;
