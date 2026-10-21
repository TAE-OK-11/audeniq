// 연결된 AI 앱 — 서버가 준 연결을 앱 이름·작업 공간·권한으로 보여 주고, 확인을 받은 뒤에만 끊는지.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ToastProvider } from './Toast';
import { ConfirmProvider } from './Confirm';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const http = vi.hoisted(() => ({ req: vi.fn(), side: vi.fn(), asks: vi.fn() }));
// AI 설정·승인 대기 목록은 따로 받는다 (각 테스트의 연결 목록 응답과 섞이지 않게)
vi.mock('../api/http', () => ({
  req: (...args: [string, unknown?]) => (args[0].includes('/ai-permission-requests') ? http.asks(...args)
    : /\/ai-(settings|submissions)/.test(args[0]) ? http.side(...args) : http.req(...args)),
  orgId: () => 'o1',
  orgPath: (p: string) => `/api/orgs/o1${p}`,
}));
vi.mock('../lib/mode', () => ({ MOCK: false }));
vi.mock('../lib/router', () => ({ Link: ({ to, children, ...rest }: { to: string; children: unknown }) => <a href={to} {...rest}>{children as never}</a> }));

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
beforeEach(() => {
  el = document.createElement('div'); document.body.appendChild(el); root = createRoot(el);
  http.side.mockImplementation((url: string) => Promise.resolve(url.includes('/ai-settings') ? { studio_approval: false } : { items: [] }));
  http.asks.mockResolvedValue({ items: [] });
});
afterEach(() => { act(() => root.unmount()); el.remove(); http.req.mockReset(); http.side.mockReset(); http.asks.mockReset(); document.body.innerHTML = ''; });

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

  it('makes an agent key, shows it once and lists it by its name', async () => {
    const token = { id: 'g3', token: 'a'.repeat(64), expires_at: '2026-11-17T00:00:00Z', scopes: ['releases:read', 'releases:write'], mcp_url: 'https://studio.audeniq.com/api/mcp' };
    const listed = { id: 'g3', client_name: '에이전트 키', client_id: 'audeniq-connection-token', org_id: 'o1', org_name: 'Personal workspace',
      scopes: token.scopes, created_at: '2026-10-18T00:00:00Z', last_used_at: null, method: 'TOKEN', label: '작업실 에이전트', expires_at: token.expires_at };
    let made = false;
    http.req.mockImplementation((url: string) => {
      if (url.endsWith('/tokens')) { made = true; return Promise.resolve(token); }
      return Promise.resolve({ items: made ? [listed] : [] });
    });
    await render();
    await act(async () => { buttons('에이전트 키 만들기')[0].click(); });
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
    expect(el.textContent).toContain('키를 다시 볼 수 없어요');
    expect([...el.querySelectorAll('li strong')].map(s => s.textContent)).toEqual(['작업실 에이전트']);
    expect(el.textContent).toContain('에이전트 키 · 내 작업 공간');
    // Closing the notice hides the token for good.
    await act(async () => { buttons('닫기')[0].click(); });
    expect(el.textContent).not.toContain('a'.repeat(64));
  });

  it('allows every permission and no end date, with a warning', async () => {
    const token = { id: 'g4', token: 'b'.repeat(64), expires_at: null, no_expiry: true, scopes: [], mcp_url: 'https://studio.audeniq.com/api/mcp' };
    http.req.mockImplementation((url: string) => Promise.resolve(url.endsWith('/tokens') ? token : { items: [] }));
    await render();
    await act(async () => { buttons('에이전트 키 만들기')[0].click(); });
    const input = el.querySelector<HTMLInputElement>('#aqTokenLabel')!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, '서버 에이전트');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });
    const all = [...el.querySelectorAll('label')].find(l => l.textContent === '모든 권한 허용')!.querySelector('input')!;
    await act(async () => { all.click(); });
    await act(async () => { buttons('무기한')[0].click(); });
    expect(buttons('무기한')[0].getAttribute('aria-checked')).toBe('true');
    expect(el.textContent).toContain('직접 끊기 전까지 계속 쓸 수 있어요');
    expect(el.textContent).toContain('내 이름으로 신청할 수 있어요');
    await act(async () => { el.querySelector('form')!.requestSubmit(); });
    expect(http.req).toHaveBeenCalledWith('/api/auth/ai-connections/tokens', {
      method: 'POST',
      body: { label: '서버 에이전트', org_id: 'o1', scopes: ['releases:write', 'releases:submit', 'settlement:read', 'payouts:request'], no_expiry: true },
    });
    expect(el.textContent).toContain('무기한으로 쓸 수 있어요');
    // Unticking "all" turns every optional permission off again.
  });

  it('approves an AI submission only after showing the declarations, and toggles Studio approval', async () => {
    const pending = [{
      id: 'p1', org_id: 'o1', org_name: 'Personal workspace', release_id: 'r1', title: 'Spring', client_name: 'Claude',
      declarations: { rights_confirmed: true, adult_confirmed: true, is_cover: false }, created_at: '2026-10-18T00:00:00Z', expires_at: '2026-10-25T00:00:00Z',
    }];
    http.req.mockResolvedValue({ items: [] });
    http.side.mockImplementation((url: string, opts?: { method?: string; body?: unknown }) => {
      if (url.endsWith('/approve')) return Promise.resolve({ status: 'APPROVED' });
      if (url.includes('/ai-settings')) return Promise.resolve({ studio_approval: opts?.method === 'PUT' ? true : false });
      return Promise.resolve({ items: pending });
    });
    await render();
    expect(el.textContent).toContain('승인을 기다리는 AI 신청');
    expect(el.textContent).toContain('Spring');
    await act(async () => { buttons('승인')[0].click(); });
    // The confirm dialog lists what will be declared in the artist's name.
    expect(document.body.textContent).toContain('만 18세 이상이에요');
    expect(document.body.textContent).not.toContain('다른 사람의 곡을 커버했어요');
    await act(async () => { buttons('승인하고 신청하기')[0].click(); });
    expect(http.side).toHaveBeenCalledWith('/api/auth/ai-submissions/p1/approve', { method: 'POST', body: {} });
    expect(el.textContent).not.toContain('승인을 기다리는 AI 신청');

    const toggle = [...el.querySelectorAll('label')].find(l => l.textContent?.includes('스튜디오에서 한 번 더 승인'))!.querySelector('input')!;
    expect(toggle.checked).toBe(false);
    await act(async () => { toggle.click(); });
    expect(http.side).toHaveBeenCalledWith('/api/auth/ai-settings', { method: 'PUT', body: { studio_approval: true } });
  });

  it('lists permission requests with a link to the approval page', async () => {
    http.req.mockResolvedValue({ items: [] });
    http.asks.mockResolvedValue({ items: [{ id: 'q1', status: 'PENDING', scopes: ['settlement:read'], reason: '정산 확인', client_name: 'Manus',
        org_id: 'o1', org_name: 'Personal workspace', created_at: '2026-10-21T00:00:00Z', expires_at: '2026-10-21T01:00:00Z', decided_at: null,
        connection_scopes: ['releases:read'], connection_label: null, connection_revoked: false }] });
    await render();
    expect(el.textContent).toContain('권한 추가 요청');
    expect(el.textContent).toContain('Manus');
    expect(el.textContent).toContain('정산 보기');
    expect(el.querySelector('a[href="/ai/approve/q1"]')).not.toBeNull();
  });

  it('keeps a just-made token on screen when the list refresh fails', async () => {
    const token = { id: 'g5', token: 'c'.repeat(64), expires_at: null, no_expiry: true, scopes: [], mcp_url: 'https://studio.audeniq.com/api/mcp' };
    let calls = 0;
    http.req.mockImplementation((url: string) => {
      if (url.endsWith('/tokens')) return Promise.resolve(token);
      calls += 1;
      return calls === 1 ? Promise.resolve({ items: [] }) : Promise.reject(new Error('network'));
    });
    await render();
    await act(async () => { buttons('에이전트 키 만들기')[0].click(); });
    const input = el.querySelector<HTMLInputElement>('#aqTokenLabel')!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, '에이전트');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });
    await act(async () => { el.querySelector('form')!.requestSubmit(); });
    expect(el.textContent).toContain('c'.repeat(64));
    expect(el.querySelector('[role="alert"]')).toBeNull();
  });

  it('hides connection tokens when MCP is off and the payout permission when the limit is 0', async () => {
    http.req.mockResolvedValue({ items: [] });
    http.side.mockImplementation((url: string) => Promise.resolve(url.includes('/ai-settings')
      ? { studio_approval: false, mcp_enabled: false, payout_scope: false } : { items: [] }));
    await render();
    expect(buttons('에이전트 키 만들기')).toHaveLength(0);
    act(() => root.unmount());
    root = createRoot(el);
    http.side.mockImplementation((url: string) => Promise.resolve(url.includes('/ai-settings')
      ? { studio_approval: false, mcp_enabled: true, payout_scope: false } : { items: [] }));
    await render();
    await act(async () => { buttons('에이전트 키 만들기')[0].click(); });
    expect(el.textContent).not.toContain('소액 지급 신청');
    // The period chips are one radio group: only the chosen one is a tab stop, arrows move it.
    const chips = [...el.querySelectorAll<HTMLButtonElement>('[role="radio"]')];
    expect(chips.map(c => c.tabIndex)).toEqual([-1, 0, -1, -1]);
    expect(chips[1].getAttribute('aria-pressed')).toBeNull();
    await act(async () => { chips[1].dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true })); });
    expect(el.querySelector('[role="radio"][aria-checked="true"]')!.textContent).toBe('90일');
  });

  it('drops an AI submission the server already settled', async () => {
    const { ApiError } = await import('../api/errors');
    const pending = [{ id: 'p2', org_id: 'o1', org_name: 'Personal workspace', release_id: 'r2', title: 'Old', client_name: 'Claude',
      declarations: { rights_confirmed: true }, created_at: '2026-10-18T00:00:00Z', expires_at: '2026-10-25T00:00:00Z' }];
    http.req.mockResolvedValue({ items: [] });
    http.side.mockImplementation((url: string) => {
      if (url.endsWith('/approve')) return Promise.reject(new ApiError('내용이 바뀌었어요', 422, 'AI_SUBMISSION_STALE'));
      if (url.includes('/ai-settings')) return Promise.resolve({ studio_approval: true });
      return Promise.resolve({ items: pending });
    });
    await render();
    await act(async () => { buttons('승인')[0].click(); });
    await act(async () => { buttons('승인하고 신청하기')[0].click(); });
    expect(el.textContent).not.toContain('승인을 기다리는 AI 신청');
  });

  it('shows a load error', async () => {
    http.req.mockRejectedValue(new Error('boom'));
    await render();
    expect(el.querySelector('[role="alert"]')).not.toBeNull();
  });
});
