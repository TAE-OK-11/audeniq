import { describe, expect, it } from 'bun:test';
import { formatBirth, validBirth } from '../src/pages/IdentityDesk';

describe('ID check input', () => {
  it('formats the birth date while typing', () => {
    expect(formatBirth('1990')).toBe('1990');
    expect(formatBirth('19900')).toBe('1990-0');
    expect(formatBirth('19900101')).toBe('1990-01-01');
    expect(formatBirth('1990-01-01')).toBe('1990-01-01');
    expect(formatBirth('1990010199')).toBe('1990-01-01');
  });
  it('accepts only real past dates', () => {
    expect(validBirth('1990-01-01')).toBe(true);
    for (const bad of ['1990-02-30', '1990-1-1', '2999-01-01', '', '19900101']) expect(validBirth(bad)).toBe(false);
  });
});
