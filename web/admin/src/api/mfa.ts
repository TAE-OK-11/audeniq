// 스태프 2단계 인증(TOTP) — 서버 crates/core/src/staff_mfa.rs
import { req } from './http';

export interface MfaStatus { required: boolean; enrolled: boolean; verified: boolean }
export interface MfaEnrollment { secret: string; otpauth_uri: string; digits: number; period: number }

export const MFA_CODES = ['STAFF_MFA_REQUIRED', 'STAFF_MFA_ENROLL_REQUIRED'] as const;

export const mfaApi = {
  status: () => req<MfaStatus>('/api/staff/mfa'),
  enroll: () => req<MfaEnrollment>('/api/staff/mfa/enroll', { method: 'POST' }),
  verify: (code: string) => req<{ verified: true }>('/api/staff/mfa/verify', { method: 'POST', body: { code } }),
};
