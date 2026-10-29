// 회원가입 직후 휴대폰 번호 인증 화면(/phone-setup)으로 넘기는 번호.
// 서버는 문자 인증을 마친 번호만 저장하고, 인증은 로그인한 뒤에만 할 수 있어 가입 요청에 실을 수 없다.
// 메모리에만 잠깐 들고 간다 — 새로고침하면 사라지고, 그때는 계정 정보에서 등록한다.
let pending: string | null = null;

export function preparePhoneHandoff(phone: string) { pending = phone; }
export const hasPhoneHandoff = () => pending !== null;
export const peekPhoneHandoff = () => pending;
export function clearPhoneHandoff() { pending = null; }
