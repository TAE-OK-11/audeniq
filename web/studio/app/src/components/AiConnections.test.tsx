// 연결된 AI 앱 — 서버가 준 연결을 앱 이름·작업 공간·권한으로 보여 주고, 확인을 받은 뒤에만 끊는지.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ToastProvider } from './Toast';
import { ConfirmProvider } from './Confirm';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const http = vi.hoisted(() => ({ req: vi.fn() }));
vi.mock('../api/http', () => ({ req: http.req, orgId: () => 'o1', orgPath: (p: string) => `/api/orgs/o1${p}` }));
vi.mock('../lib/mode', () => ({ MOCK: false }));

import { AiConnections } from './AiConnections';

const items = [
  {
    id: 'g1', client_name: 'ChatGPT', client_id: 'https://chatgpt.com/oauth/client.json', org_id: 'o1', org_name: 'Personal workspace',
    scopes: ['releases:read', 'releases:write', 'payouts:request'], created_at: '2026-10-01T00:00:00Z', last_used_at: null,
  },
  {
    id: 'g2', client_name: 'Claude', client_id: 'audeniq-mcp-abc', org_id: 'o1', org_name: 'Han River Label',
    scopes: ['releases:read'], created_at: '2026-10-02T00:00:00Z', last_used_at: '2026-10-03T00:00:00Z',
  },
];

let root: Root; let el: HTMLDivElement;
beforeEach(() => { el = document.createElement('div'); document.body.appendChild(el); root = createRoot(el); });
afterEach(() => { act(() => root.unmount()); el.remove(); http.req.mockReset(); document.body.innerHTML = ''; });

async function render() {
  await act(async () => {
    root.render(<ToastProvider><ConfirmProvider><AiConnections /></ConfirmProvider></ToastProvider>);
  });
}
const buttons = (label: string) => [...document.querySelectorAll('button')].filter(b => b.textContent === label);

describe('AiConnections', () => {
  it('lists each app with its host, workspace and permissions', async () => {
    http.req.mockResolvedValue({ items });
    await render();
    expect(http.req).toHaveBeenCalledWith('/api/auth/ai-connections');
    const text = el.textContent ?? '';
    expect(text).toContain('ChatGPT');
    expect(text).toContain('chatgpt.com');
    expect(text).toContain('내 작업 공간');
    expect(text).toContain('발매 정보 보기, 발매 초안 작성·수정, 소액 지급 신청');
    expect(text).toContain('아직 사용 안 함');
    expect(text).toContain('Han River Label');
    expect(text).not.toContain('audeniq-mcp-abc');
  });

  it('shows the empty state', async () => {
    http.req.mockResolvedValue({ items: [] });
    await render();
    expect(el.textContent).toContain('연결된 AI 앱이 없어요');
  });

  it('revokes only after confirmation and removes the row', async () => {
    http.req.mockImplementation((url: string) => Promise.resolve(url.endsWith('/revoke') ? { revoked: true } : { items }));
    await render();
    await act(async () => { buttons('연결 끊기')[0].click(); });
    // The confirm dialog: closing it revokes nothing.
    await act(async () => { buttons('닫기')[0].click(); });
    expect(http.req).not.toHaveBeenCalledWith(expect.stringContaining('/revoke'), expect.anything());
    await act(async () => { buttons('연결 끊기')[0].click(); });
    const confirmButton = buttons('연결 끊기').find(b => b.className.includes('is-danger-solid'))!;
    await act(async () => { confirmButton.click(); });
    expect(http.req).toHaveBeenCalledWith('/api/auth/ai-connections/g1/revoke', { method: 'POST', body: {} });
    const rows = [...el.querySelectorAll('li strong')].map(s => s.textContent);
    expect(rows).toEqual(['Claude']);
  });

  it('makes a connection token, shows it once and lists it by its name', async () => {
    const token = { id: 'g3', token: 'a'.repeat(64), expires_at: '2026-11-17T00:00:00Z', scopes: ['releases:read', 'releases:write'], mcp_url: 'https://studio.audeniq.com/api/mcp' };
    const listed = { id: 'g3', client_name: '연결 토큰', client_id: 'audeniq-connection-token', org_id: 'o1', org_name: 'Personal workspace',
      scopes: token.scopes, created_at: '2026-10-18T00:00:00Z', last_used_at: null, method: 'TOKEN', label: '작업실 에이전트', expires_at: token.expires_at };
    let made = false;
    http.req.mockImplementation((url: string) => {
      if (url.endsWith('/tokens')) { made = true; return Promise.resolve(token); }
      return Promise.resolve({ items: made ? [listed] : [] });
    });
    await render();
    await act(async () => { buttons('연결 토큰 만들기')[0].click(); });
    const input = el.querySelector<HTMLInputElement>('#aqTokenLabel')!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, '작업실 에이전트');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });
    await act(async () => { el.querySelector('form')!.requestSubmit(); });
    expect(http.req).toHaveBeenCalledWith('/api/auth/ai-connections/tokens', {
      method: 'POST', body: { label: '작업실 에이전트', org_id: 'o1', scopes: ['releases:write'], days: 30 },
    });
    expect(el.textContent).toContain('a'.repeat(64));
    expect(el.textContent).toContain('다시 볼 수 없으니');
    expect([...el.querySelectorAll('li strong')].map(s => s.textContent)).toEqual(['작업실 에이전트']);
    expect(el.textContent).toContain('연결 토큰 · 내 작업 공간');
    // Closing the notice hides the token for good.
    await act(async () => { buttons('닫기')[0].click(); });
    expect(el.textContent).not.toContain('a'.repeat(64));
  });

  it('shows a load error', async () => {
    http.req.mockRejectedValue(new Error('boom'));
    await render();
    expect(el.querySelector('[role="alert"]')).not.toBeNull();
  });
});
