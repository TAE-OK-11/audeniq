// 계정 정보(본인 확인·납세) 공유 스토어 — 계정 정보 화면과 수령 계좌 등록 흐름이 함께 쓴다.
// 본인인증은 화면 흐름만 있는 체험 단계라 실제 인증 기관에 요청하지 않는다.
// 서버 저장 항목이 아직 없어 실서버 모드에선 브라우저에 남기지 않고(새로고침하면 비워짐) 체험 모드에서만 저장한다.
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
}

export interface AccountInfo {
  realName: string;
  verified: { method: VerifyMethod; at: string } | null;
  tax: TaxResidency | null;
}

const EMPTY: AccountInfo = { realName: '', verified: null, tax: null };

const store = createStore<AccountInfo>(EMPTY, {
  persist: MOCK ? 'account' : undefined,
  revive: (raw, fallback) => (raw && typeof raw === 'object' ? { ...fallback, ...(raw as Partial<AccountInfo>) } : fallback),
});

export const useAccount = store.use;
export const getAccountSnapshot = store.get;
export function patchAccount(p: Partial<AccountInfo>): void {
  store.set(prev => ({ ...prev, ...p }));
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

export function taxCountryLabel(code: string): string {
  return TAX_COUNTRIES.find(c => c.code === code)?.label ?? code;
}
