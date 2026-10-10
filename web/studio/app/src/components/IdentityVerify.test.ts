import { describe, expect, it } from 'vitest';
import { birthFrom } from './IdentityVerify';

describe('birthFrom', () => {
  it('maps the first back digit to the century', () => {
    expect(birthFrom('900101', '1')).toBe('1990-01-01');
    expect(birthFrom('050303', '4')).toBe('2005-03-03');
    expect(birthFrom('991231', '9')).toBe('1899-12-31');
  });
  it('rejects impossible dates and bad input', () => {
    expect(birthFrom('010229', '3')).toBeNull();
    expect(birthFrom('001332', '1')).toBeNull();
    expect(birthFrom('9001', '1')).toBeNull();
    expect(birthFrom('900101', '')).toBeNull();
  });
});
