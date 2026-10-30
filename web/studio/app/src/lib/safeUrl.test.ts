import { describe, expect, it } from 'vitest';
import { cssUrl, navigableUrl, webUrl } from './safeUrl';

describe('safeUrl', () => {
  it('스크립트를 실행하는 주소는 막는다', () => {
    for (const u of ['javascript:alert(1)', ' JavaScript:alert(1)', 'java\tscript:alert(1)', 'data:text/html,<script>', 'vbscript:x', 'blob:https://a/b']) {
      expect(navigableUrl(u)).toBeUndefined();
    }
  });
  it('일반 주소와 앱 주소는 그대로', () => {
    expect(navigableUrl('https://claude.ai/cb?code=1')).toBe('https://claude.ai/cb?code=1');
    expect(navigableUrl('claude://oauth/cb')).toBe('claude://oauth/cb');
    expect(navigableUrl('/releases/1')).toBe('/releases/1');
  });
  it('webUrl은 http(s)만', () => {
    expect(webUrl('https://a.b/c')).toBe('https://a.b/c');
    expect(webUrl('claude://x')).toBeUndefined();
    expect(webUrl('/x')).toBeUndefined();
  });
});

describe('cssUrl', () => {
  it('따옴표로 값을 끊지 못한다', () => {
    expect(cssUrl('https://a/b.jpg')).toBe('url("https://a/b.jpg")');
    expect(cssUrl('https://a/"),url(x')).toBe('url("https://a/%22),url(x")');
  });
});
