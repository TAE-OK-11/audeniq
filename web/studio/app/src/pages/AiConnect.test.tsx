// AI 앱 연결 화면 — 요청을 보여 주고, 고른 작업 공간·권한만 보내고, 돈·접수 권한은 한 번 더 묻고,
// 기기 연결 코드와 열 수 없는 요청을 알맞게 안내하는지.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ApiError } from '../api/errors';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const h = vi.hoisted(() => ({ req: vi.fn(), search: '' }));
vi.mock('../api/http', () => ({ req: (...a: [string, unknown?]) => h.req(...a), orgId: () => 'o1', orgPath: (p: string) => p }));
vi.mock('../lib/mode', () => ({ MOCK: false }));
vi.mock('../api/auth', () => ({ useAuth: () => ({ user: { email: 'hana@example.com' } }) }));
vi.mock('../lib/router', () => ({
  Link: ({ to, children, ...rest }: { to: string; children: unknown }) => <a href={to} {...rest}>{children as never}</a>,
  useLocation: () => ({ search: h.search }),
}));

import { AiConnect } from './AiConnect';

const details = {
  kind: 'browser',
  client: { name: 'Claude', client_id: 'https://claude.ai/oauth/mcp-oauth-client-metadata', verified_host: 'claude.ai', registered: false },
  requested_from: 'claude.ai', return_to: 'claude.ai', device_code: null,
  scopes: [
    { scope: 'releases:read', title: '발매 정보 보기', description: '', locked: true, checked: true, risk: 'none' },
    { scope: 'releases:write', title: '발매 초안 작성·수정', description: '', locked: false, checked: true, risk: 'none' },
    { scope: 'payouts:request', title: '소액 지급 신청', description: '', locked: false, checked: false, risk: 'money' },
  ],
  workspaces: [{ id: 'o1', name: 'Personal workspace', role: 'OWNER' }, { id: 'o2', name: 'Han River Label', role: 'EDITOR' }],
  expires_at: new Date(Date.now() + 600_000).toISOString(),
};

let root: Root; let el: HTMLDivElement;
beforeEach(() => { el = document.createElement('div'); document.body.appendChild(el); root = createRoot(el); });
afterEach(() => { act(() => root.unmount()); el.remove(); h.req.mockReset(); });

async function render(search: string) {
  h.search = search;
  await act(async () => { root.render(<AiConnect />); });
}
const click = async (node: Element | null | undefined) => { await act(async () => { (node as HTMLElement).click(); }); };
const button = (text: string) => [...el.querySelectorAll('button')].find(b => b.textContent?.includes(text));

describe('AI app connection page', () => {
  it('shows who asks and sends only the chosen workspace and permissions', async () => {
    h.req.mockImplementation((url: string) => (url.startsWith('/api/auth/ai-connect?')
      ? Promise.resolve(details)
      : Promise.resolve({ kind: 'browser', status: 'APPROVED', redirect: 'https://claude.ai/cb?code=x', scopes: ['releases:read'] })));
    await render('?request=abc');
    expect(h.req.mock.calls[0][0]).toBe('/api/auth/ai-connect?request=abc');
    expect(el.textContent).toContain('Claude에서 어데닉을 쓸 수 있게 할까요?');
    expect(el.textContent).toContain('claude.ai에서 확인된 앱');
    expect(el.textContent).toContain('hana@example.com');
    expect(el.textContent).toContain('어떤 권한을 줘도 지켜져요');
    await click(el.querySelector('[role=radio]:nth-child(2)'));
    await click(button('보기만'));
    await click(button('허용하고 연결'));
    const [url, opts] = h.req.mock.calls[1];
    expect(url).toBe('/api/auth/ai-connect');
    expect((opts as { body: unknown }).body).toEqual({ request: 'abc', decision: 'approve', org: 'o2', scopes: ['releases:read'] });
    expect(el.textContent).toContain('연결했어요');
    expect(el.querySelector('a[href="https://claude.ai/cb?code=x"]')).not.toBeNull();
  });

  it('asks once more before giving payout permission', async () => {
    h.req.mockResolvedValueOnce(details).mockResolvedValueOnce({ kind: 'browser', status: 'APPROVED', scopes: [] });
    await render('?request=abc');
    await click(button('모두'));
    await click(button('허용하고 연결'));
    expect(h.req).toHaveBeenCalledTimes(1);
    expect(el.textContent).toContain('소액 지급 신청 권한도 줄까요?');
    await click(button('네, 허용하고 연결'));
    expect(h.req).toHaveBeenCalledTimes(2);
  });

  it('turns a device code into the request', async () => {
    h.req.mockResolvedValueOnce({ request: 'dev-req' }).mockResolvedValueOnce({ ...details, kind: 'device', device_code: 'BCDF-GHJK' });
    await render('?user_code=BCDF-GHJK');
    expect(h.req.mock.calls[0][0]).toBe('/api/auth/ai-connect/device');
    expect(h.req.mock.calls[1][0]).toBe('/api/auth/ai-connect?request=dev-req');
    expect(el.textContent).toContain('AI 앱에 보이는 코드와 같은지 확인해 주세요');
    expect(el.textContent).toContain('BCDF-GHJK');
  });

  it('explains a request that can no longer be opened', async () => {
    h.req.mockRejectedValue(new ApiError('시간이 지났어요', 400, 'AI_CONNECT_EXPIRED'));
    await render('?request=old');
    expect(el.textContent).toContain('연결 요청을 열 수 없어요');
    expect(el.textContent).toContain('연결 요청 시간(10분)이 지났어요.');
  });

  it('asks for the code when none was given', async () => {
    await render('?device=1');
    expect(el.textContent).toContain('AI 앱에 보이는 연결 코드를 입력해 주세요');
    expect(h.req).not.toHaveBeenCalled();
  });
});
