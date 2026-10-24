import { describe, expect, it } from 'vitest';
import { embedHtml } from './embed';

describe('임베드 카드', () => {
  it('글자를 이스케이프하고 스마트링크로 연결', () => {
    const html = embedHtml({ url: 'https://studio.audeniq.com/l/abcd2345', title: '<b>"여름"</b>', artist: 'A & B', released: true });
    expect(html).toContain('href="https://studio.audeniq.com/l/abcd2345"');
    expect(html).toContain('&lt;b&gt;&quot;여름&quot;&lt;/b&gt;');
    expect(html).toContain('A &amp; B');
    expect(html).not.toContain('<b>');
    expect(html).toContain('>듣기<');
  });
});
