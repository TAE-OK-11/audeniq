// 휴대폰 번호 등록과 문자 인증 비밀번호 재설정 (crates/core/src/password_reset.rs).
// 체험 모드에는 문자 발송이 없어 시작 단계에서 바로 알린다.
import { ApiError } from './errors';
import { req } from './http';
import { MOCK } from '../lib/mode';

export interface SmsChallenge { id: string; expiresAt: string; phoneLast4: string }

const mockReject = () => Promise.reject(new ApiError('체험 모드에서는 문자 인증을 쓸 수 없어요.', 400, 'SMS_UNSUPPORTED'));

/** 1. 이메일 계정에 등록된 번호와 같을 때만 문자가 간다 — 다르거나 없는 계정이어도 같은 모양으로 답한다 */
export async function startPasswordReset(email: string, phone: string): Promise<SmsChallenge> {
  if (MOCK) return mockReject();
  const r = await req<{ reset_id: string; expires_at: string; phone_last4: string }>('/api/auth/password-reset/start', {
    method: 'POST', body: { email: email.trim(), phone }, quiet401: true,
  });
  return { id: r.reset_id, expiresAt: r.expires_at, phoneLast4: r.phone_last4 };
}

/** 2. 인증번호가 맞으면 15분짜리 재설정 토큰 */
export async function verifyPasswordReset(resetId: string, code: string): Promise<string> {
  const r = await req<{ reset_token: string }>('/api/auth/password-reset/verify', {
    method: 'POST', body: { reset_id: resetId, code }, quiet401: true,
  });
  return r.reset_token;
}

/** 3. 비밀번호를 바꾸고 모든 세션을 끊는다 — 다시 로그인해야 한다 */
export async function completePasswordReset(resetToken: string, newPassword: string): Promise<void> {
  await req('/api/auth/password-reset/complete', {
    method: 'POST', body: { reset_token: resetToken, new_password: newPassword }, quiet401: true,
  });
}

/** 계정에 휴대폰 번호 등록 — 인증번호 문자 발송 */
export async function startPhoneRegistration(phone: string): Promise<SmsChallenge> {
  if (MOCK) return mockReject();
  const r = await req<{ challenge_id: string; expires_at: string; phone_last4: string }>('/api/me/phone/start', {
    method: 'POST', body: { phone },
  });
  return { id: r.challenge_id, expiresAt: r.expires_at, phoneLast4: r.phone_last4 };
}

export async function verifyPhoneRegistration(challengeId: string, code: string): Promise<{ last4: string; verifiedAt: string }> {
  const r = await req<{ phone: { last4: string; verified_at: string } }>('/api/me/phone/verify', {
    method: 'POST', body: { challenge_id: challengeId, code },
  });
  return { last4: r.phone.last4, verifiedAt: r.phone.verified_at };
}
