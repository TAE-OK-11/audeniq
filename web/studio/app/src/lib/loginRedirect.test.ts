import { describe, expect, it } from 'vitest';
import { loginRedirect } from './loginRedirect';

describe('persisted login redirect', () => {
  it('keeps the requested page and supports a stored Location object', () => {
    expect(loginRedirect({ from: '/admin/content?tab=events' })).toBe('/admin/content?tab=events');
    expect(loginRedirect({ from: { pathname: '/releases/r1', search: '?tab=tracks', hash: '#track' } })).toBe('/releases/r1?tab=tracks#track');
  });
  it('ignores malformed history without throwing or sending the user to an auth loop', () => {
    for (const from of [null, true, 7, {}, { pathname: 7 }, { pathname: '/admin', search: {} }, '/login', '/signup?x=1', '/find-account#reset']) {
      expect(loginRedirect({ from })).toBeUndefined();
    }
    expect(loginRedirect(null)).toBeUndefined();
    expect(loginRedirect('invalid')).toBeUndefined();
  });
  it('accepts only an internal page path', () => {
    for (const from of ['https://example.test', '//example.test', '/\\example.test', '/admin\n', 'admin']) {
      expect(loginRedirect({ from })).toBeUndefined();
    }
  });
});
