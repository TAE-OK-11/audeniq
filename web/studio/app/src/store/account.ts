// 계정 정보(본인 확인·납세) 공유 스토어 — 계정 정보 화면과 수령 계좌 등록 흐름이 함께 쓴다.
// 실서버 모드: 로그인하면 /api/me/account에서 채우고(본인인증은 서버가 인증 기관 결과를 확인해 저장),
// 브라우저에는 남기지 않는다. 체험 모드에서만 화면 흐름 결과를 브라우저에 저장한다.
import { createStore } from '../lib/store';
import { MOCK } from '../lib/mode';

export type VerifyMethod = 'pass' | 'sms' | 'ipin' | 'card';

export const VERIFY_LABEL: Record<VerifyMethod, string> = {
  pass: 'PASS',
  sms: '휴대폰 문자',
  ipin: '아이핀',
  card: '카드사',
};

export interface TaxResidency {
  /** 대한민국 외 국가에도 납세 의무가 있는지 */
  foreign: boolean;
  /** 납세 의무가 있는 국가(foreign일 때) */
  country: string;
  /** 해당 국가 납세자번호(TIN) — 선택 */
  tin: string;
  /** 미국 시민권·영주권 여부(미국 선택 시) */
  usPerson: boolean;
  /** 서버에 저장된 납세자번호 뒷자리 (실서버 모드, 번호 전체는 받지 않는다) */
  tinLast4?: string | null;
}

export interface AccountInfo {
  realName: string;
  /** method: 체험 모드는 고른 방법, 실서버는 인증 기관이 확인한 방법. adult: 만 19세 이상(실서버) */
  verified: { method: VerifyMethod | string; at: string; adult?: boolean } | null;
  tax: TaxResidency | null;
  /** 본인인증 기관 연결 상태 (실서버) — name 'test'는 개발·테스트 환경 */
  provider?: { ready: boolean; name: string | null };
}

export const EMPTY_ACCOUNT: AccountInfo = { realName: '', verified: null, tax: null };
const EMPTY = EMPTY_ACCOUNT;

const store = createStore<AccountInfo>(EMPTY, {
  persist: MOCK ? 'account' : undefined,
  revive: (raw, fallback) => (raw && typeof raw === 'object' ? { ...fallback, ...(raw as Partial<AccountInfo>) } : fallback),
});

export const useAccount = store.use;
export const getAccountSnapshot = store.get;
export function patchAccount(p: Partial<AccountInfo>): void {
  store.set(prev => ({ ...prev, ...p }));
}
export function setAccount(a: AccountInfo): void {
  store.set(a);
}

export const TAX_COUNTRIES: { code: string; label: string }[] = [
  { code: 'US', label: '미국' },
  { code: 'JP', label: '일본' },
  { code: 'CN', label: '중국' },
  { code: 'GB', label: '영국' },
  { code: 'CA', label: '캐나다' },
  { code: 'AU', label: '호주' },
  { code: 'DE', label: '독일' },
  { code: 'FR', label: '프랑스' },
  { code: 'SG', label: '싱가포르' },
  { code: 'OTHER', label: '기타' },
];

/** 인증을 마친 방법 — 본인인증 기관 방법과 직원 신분증 확인(crates/core/src/identity_desk.rs).
 *  서버 값은 소문자로 바뀌어 온다(api/portal.ts): IN_PERSON → in_person */
const VERIFIED_BY: Record<string, string> = {
  pass: 'PASS로 인증', sms: '문자로 인증', ipin: '아이핀으로 인증', card: '카드사로 인증',
  in_person: '대면 인증', customer_center: '고객센터를 통한 인증',
};

/** 인증 방법 표시 문구 — 모르는 방법(예: TEST)은 이름 그대로 */
export function verifyLabel(method: string): string {
  return VERIFIED_BY[method.toLowerCase()] ?? `${method} 인증`;
}

export function taxCountryLabel(code: string): string {
  return TAX_COUNTRIES.find(c => c.code === code)?.label ?? code;
}
