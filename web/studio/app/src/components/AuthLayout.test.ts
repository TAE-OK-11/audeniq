import { describe, expect, it } from 'vitest';
import { emailSuggestion } from './AuthLayout';

describe('emailSuggestion', () => {
  it('자주 틀리는 도메인을 바로잡는다', () => {
    expect(emailSuggestion('kim@gamil.com')).toBe('kim@gmail.com');
    expect(emailSuggestion('kim@gmail.co')).toBe('kim@gmail.com');
    expect(emailSuggestion('Lee@Naver.con')).toBe('lee@naver.com');
    expect(emailSuggestion('park@hanmial.net')).toBe('park@hanmail.net');
  });
  it('맞는 주소나 회사 도메인은 건드리지 않는다', () => {
    expect(emailSuggestion('kim@gmail.com')).toBe('');
    expect(emailSuggestion('kim@audeniq.com')).toBe('');
    expect(emailSuggestion('kim@label.co.kr')).toBe('');
    expect(emailSuggestion('kim@')).toBe('');
    expect(emailSuggestion('not-an-email')).toBe('');
  });
});
