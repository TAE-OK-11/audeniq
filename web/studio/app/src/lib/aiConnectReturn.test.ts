import { beforeEach, describe, expect, it } from 'vitest';
import { aiConnectPath, rememberAiConnectReturn, takeAiConnectReturn } from './aiConnectReturn';

describe('AI connect return path', () => {
  beforeEach(() => sessionStorage.clear());

  it('accepts only the consent and device pages of this site', () => {
    expect(aiConnectPath('/api/oauth/authorize?client_id=https%3A%2F%2Fclaude.ai%2Fx&state=s')).toBe('/api/oauth/authorize?client_id=https%3A%2F%2Fclaude.ai%2Fx&state=s');
    expect(aiConnectPath('/api/oauth/device')).toBe('/api/oauth/device');
    expect(aiConnectPath('/api/oauth/device?user_code=BCDF-GHJK')).toBe('/api/oauth/device?user_code=BCDF-GHJK');
    for (const bad of [undefined, null, 7, '', '/', '/releases', '/api/oauth/token', '/api/oauth/authorize',
      '/api/oauth/deviceX', 'https://evil.test/api/oauth/authorize?x', '//evil.test/api/oauth/authorize?x',
      '/api/oauth/authorize?x=//evil', '/api/oauth/authorize?x\\y', '/api/oauth/authorize?a b', '/api/oauth/authorize?\n',
      '/api/oauth/authorize?' + 'a'.repeat(5000)]) {
      expect(aiConnectPath(bad)).toBeUndefined();
    }
  });

  it('remembers ?next= once', () => {
    rememberAiConnectReturn('?next=' + encodeURIComponent('/api/oauth/authorize?state=1'));
    expect(takeAiConnectReturn()).toBe('/api/oauth/authorize?state=1');
    expect(takeAiConnectReturn()).toBeUndefined();
  });

  it('ignores other next values', () => {
    rememberAiConnectReturn('?next=' + encodeURIComponent('/releases'));
    expect(takeAiConnectReturn()).toBeUndefined();
  });
});
