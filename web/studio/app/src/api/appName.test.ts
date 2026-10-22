// AI 앱 주소 → 이름: 아는 앱만 이름으로, 비슷하게 꾸민 주소는 그대로 보여 주는지
import { describe, expect, it } from 'vitest';
import { appName, isKnownApp } from './aiConnections';

describe('appName', () => {
  it('names well-known AI apps', () => {
    expect(appName('claude.ai')).toBe('Claude');
    expect(appName('chatgpt.com')).toBe('ChatGPT');
    expect(appName('www.perplexity.ai')).toBe('Perplexity');
    expect(appName('grok.com')).toBe('Grok');
    expect(appName('gemini.google.com')).toBe('Gemini');
  });
  it('keeps unknown or lookalike hosts as they are', () => {
    expect(appName('claude.ai.evil.com')).toBe('claude.ai.evil.com');
    expect(appName('notchatgpt.com')).toBe('notchatgpt.com');
    expect(isKnownApp('agent.example.com')).toBe(false);
  });
});

describe('AI app logos', () => {
  it('uses a logo only for a verified known host or an exact known name', async () => {
    const { appBrandForHost, appBrandForName } = await import('./aiConnections');
    expect(appBrandForHost('claude.ai')).toBe('claude');
    expect(appBrandForHost('chatgpt.com')).toBe('chatgpt');
    expect(appBrandForHost('grok.com')).toBe('grok');
    expect(appBrandForHost('claude.ai.evil.com')).toBeNull();
    expect(appBrandForHost(null)).toBeNull();
    expect(appBrandForName('Perplexity')).toBe('perplexity');
    expect(appBrandForName('Claude Helper')).toBeNull();
    expect(appBrandForName('Hermes Agent')).toBe('hermes');
    expect(appBrandForName('OpenClaw')).toBe('openclaw');
    expect(appBrandForHost('muse.ai')).toBe('muse');
    expect(appBrandForHost('pickle.com')).toBe('pickle');
    expect(appBrandForHost('hermes-agent.nousresearch.com')).toBe('hermes');
  });
});
