import { describe, expect, it } from 'vitest';
import { ApiError, authErrorMessage } from './errors';

describe('authErrorMessage', () => {
  it('turns server codes into login-screen wording', () => {
    expect(authErrorMessage(new ApiError('로그인이 필요해요.', 401, 'UNAUTHENTICATED'), 'login')).toBe('이메일 또는 비밀번호가 맞지 않아요. 다시 확인해 주세요.');
    expect(authErrorMessage(new ApiError('입력값을 확인해 주세요.', 400, 'INVALID_INPUT'), 'login')).toBe('이메일 또는 비밀번호가 맞지 않아요. 다시 확인해 주세요.');
  });
  it('explains signup conflicts and keeps other messages', () => {
    expect(authErrorMessage(new ApiError('x', 409, 'CONFLICT'), 'signup')).toContain('이미 가입된 이메일');
    expect(authErrorMessage(new ApiError('네트워크 연결을 확인해 주세요.', 0, 'NETWORK'), 'login')).toBe('네트워크 연결을 확인해 주세요.');
  });
});
