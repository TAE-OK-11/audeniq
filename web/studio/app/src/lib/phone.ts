// 국내 휴대폰 번호 입력 — 회원가입·비밀번호 재설정·휴대폰 번호 등록이 함께 쓴다.

/** 휴대폰 번호를 입력하는 대로 010-1234-5678 꼴로 — 숫자만 남겨 11자리까지 */
export function formatPhone(v: string): string {
  const d = v.replace(/\D/g, '').slice(0, 11);
  if (d.length < 4) return d;
  if (d.length < 8) return `${d.slice(0, 3)}-${d.slice(3)}`;
  return `${d.slice(0, 3)}-${d.slice(3, d.length - 4)}-${d.slice(-4)}`;
}
/** 서버(crates/core/src/sms.rs normalize_phone)와 같은 규칙 — 010은 11자리, 011·016~019는 10~11자리 */
export const isPhone = (v: string) => /^(010\d{8}|01[16789]\d{7,8})$/.test(v.replace(/\D/g, ''));
