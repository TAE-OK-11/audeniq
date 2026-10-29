// 비밀번호 로그인·가입 직후 패스키 만들기 화면(/passkey-setup)으로 넘기는 값.
// 패스키 등록은 지금 비밀번호를 한 번 더 확인하므로 방금 입력한 비밀번호를 메모리에만 잠깐 들고 간다
// (주소·방문 기록·저장소에는 남기지 않는다). 새로고침하면 사라지고, 그때는 그냥 다음 화면으로 간다.
import { passkeySupported } from '../api/passkey';
import { MOCK } from '../api/client';

let pending: { password: string; next: string; signup?: boolean } | null = null;

/** 로그인·가입을 보내기 직전에 — 패스키를 쓸 수 있는 기기에서만. signup이면 패스키 화면이 가입의 마지막 단계로 보인다 */
export function preparePasskeyHandoff(password: string, next: string, signup = false) {
  pending = !MOCK && passkeySupported() ? { password, next, signup } : null;
}
export const hasPasskeyHandoff = () => pending !== null;
export const peekPasskeyHandoff = () => pending;
export function clearPasskeyHandoff() { pending = null; }
