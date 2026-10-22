// 개발자 앱 — 목록·상태를 보이고, 만들면 비밀 키를 한 번만 보여 주고, 지우기 전에 한 번 더 묻는지.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ToastProvider } from './Toast';
import { ConfirmProvider } from './Confirm';
import { ApiError } from '../api/errors';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const h = vi.hoisted(() => ({ req: vi.fn() }));
vi.mock('../api/http', () => ({ req: (...a: [string, unknown?]) => h.req(...a), orgId: () => 'o1', orgPath: (p: string) => p }));

import { DeveloperApps } from './DeveloperApps';

const info = (items: unknown[]) => ({
  items, limit: 10, api_base: 'https://studio.example/api/v1', resource: 'https://studio.example/api/v1',
  authorization_endpoint: 'https://studio.example/api/oauth/authorize', token_endpoint: 'https://studio.example/api/oauth/token',
  openapi: 'https://studio.example/api/v1/openapi.json',
});
const app = (over: Record<string, unknown> = {}) => ({
  id: 'a1', client_id: 'audeniq-app-1', name: 'Release Helper', description: null, website: null,
  redirect_uris: ['https://myapp.example/cb'], public_client: false, visibility: 'PRIVATE', review_note: null,
  review_requested_at: null, reviewed_at: null, created_at: new Date().toISOString(), connections: 0, ...over,
});

let root: Root; let el: HTMLDivElement;
beforeEach(() => { el = document.createElement('div'); document.body.appendChild(el); root = createRoot(el); });
afterEach(() => { act(() => root.unmount()); el.remove(); h.req.mockReset(); });

async function render() {
  await act(async () => { root.render(<ToastProvider><ConfirmProvider><DeveloperApps /></ConfirmProvider></ToastProvider>); });
}
const click = async (n: Element | null | undefined) => { await act(async () => { (n as HTMLElement).click(); }); };
const button = (text: string) => [...document.querySelectorAll('button')].find(b => b.textContent?.includes(text));
const type = async (sel: string, value: string) => {
  const input = el.querySelector(sel) as HTMLInputElement | HTMLTextAreaElement;
  const setter = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(input), 'value')!.set!;
  await act(async () => { setter.call(input, value); input.dispatchEvent(new Event('input', { bubbles: true })); });
};

describe('Developer apps', () => {
  it('lists apps with their review status and connection info', async () => {
    h.req.mockResolvedValueOnce(info([app({ visibility: 'REVIEW', connections: 2 })]));
    await render();
    expect(el.textContent).toContain('Release Helper');
    expect(el.textContent).toContain('검토 중');
    expect(el.textContent).toContain('2명 연결');
    expect(el.textContent).toContain('https://studio.example/api/v1');
    expect(el.querySelector('a[href="https://studio.example/api/v1/openapi.json"]')).not.toBeNull();
  });

  it('shows the secret once after making an app', async () => {
    h.req.mockResolvedValueOnce(info([])).mockResolvedValueOnce({ ...app(), client_secret: 's3cret' });
    await render();
    expect(el.textContent).toContain('아직 만든 앱이 없어요');
    await click(button('앱 만들기'));
    await type('#aqAppName', 'Release Helper');
    await type('#aqAppRedirects', 'https://myapp.example/cb\n\nhttps://myapp.example/cb2');
    await click(el.querySelector('form button[type=submit]'));
    const [url, opts] = h.req.mock.calls[1];
    expect(url).toBe('/api/auth/developer-apps');
    expect((opts as { body: { redirect_uris: string[] } }).body.redirect_uris).toEqual(['https://myapp.example/cb', 'https://myapp.example/cb2']);
    expect(el.textContent).toContain('s3cret');
    expect(el.textContent).toContain('다시 볼 수 없어요');
    await click(button('닫기'));
    expect(el.textContent).not.toContain('s3cret');
    expect(el.textContent).toContain('나만 연결');
  });

  it('asks before deleting and before a new secret', async () => {
    h.req.mockResolvedValueOnce(info([app({ connections: 3 })]));
    await render();
    await click(button('관리'));
    await click(button('앱 지우기'));
    expect(document.body.textContent).toContain('3명의 연결이 모두 끊겨요');
    expect(h.req).toHaveBeenCalledTimes(1);
  });

  it('says when developer apps are not available', async () => {
    h.req.mockRejectedValueOnce(new ApiError('없음', 404, 'NOT_FOUND'));
    await render();
    expect(el.textContent).toContain('지금은 개발자 앱을 쓸 수 없어요.');
  });
});
