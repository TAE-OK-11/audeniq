// AI 앱 연결 화면 — 요청을 보여 주고, 고른 작업 공간·권한만 보내고, 돈·접수 권한은 한 번 더 묻고,
// 기기 연결 코드와 열 수 없는 요청을 알맞게 안내하는지.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ApiError } from '../api/errors';
import { ConfirmProvider } from '../components/Confirm';

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
  await act(async () => { root.render(<ConfirmProvider><AiConnect /></ConfirmProvider>); });
}
const click = async (node: Element | null | undefined) => { await act(async () => { (node as HTMLElement).click(); }); };
const button = (text: string) => [...el.querySelectorAll('button')].find(b => b.textContent?.includes(text));

const confirmDialog = async () => {
  const ok = [...document.querySelectorAll('[role=dialog] button')].find(b => b.className.includes('is-primary'));
  await click(ok);
};

describe('AI app connection page', () => {
  it('shows who asks, then sends only the chosen workspace and permissions', async () => {
    h.req.mockImplementation((url: string) => (url.startsWith('/api/auth/ai-connect?')
      ? Promise.resolve(details)
      : Promise.resolve({ kind: 'browser', status: 'APPROVED', redirect: 'https://claude.ai/cb?code=x', scopes: ['releases:read'] })));
    await render('?request=abc');
    expect(h.req.mock.calls[0][0]).toBe('/api/auth/ai-connect?request=abc');
    // 한 화면에 요청한 앱·작업 공간·권한·지켜지는 것이 다 있어요
    expect(el.textContent).toContain('AUDENIQ을 쓰려고 해요');
    expect(el.textContent).toContain('Claude 공식 앱');
    expect(el.textContent).not.toContain('claude.ai에서');
    expect(el.textContent).toContain('hana@example.com');
    expect(el.textContent).toContain('어떤 권한을 줘도 지켜져요');
    expect(el.querySelector('.aq-wiz-progress, .wizard-progress')).toBeNull();
    await click(el.querySelectorAll('input[name=aqConnectOrg]')[1]);
    // 항상 포함되는 보기 권한은 고르는 칸이 아니라 안내 한 줄
    expect(el.querySelectorAll('.aq-option input[type=checkbox]')).toHaveLength(2);
    expect(el.textContent).toContain('발매 정보 보기는 항상 포함돼요.');
    await click(button('보기만'));
    expect(el.querySelector('#wizardNext')?.textContent).toBe('보기만 허용하고 연결');
    await click(el.querySelector('#wizardNext'));
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
    await click(el.querySelector('#wizardNext'));
    expect(h.req).toHaveBeenCalledTimes(1);
    expect(document.body.textContent).toContain('소액 지급 신청 권한도 줄까요?');
    await confirmDialog();
    expect(h.req).toHaveBeenCalledTimes(2);
  });

  it('denies after a confirmation', async () => {
    h.req.mockResolvedValueOnce(details).mockResolvedValueOnce({ kind: 'browser', status: 'DENIED', redirect: 'https://claude.ai/cb?error=access_denied', scopes: [] });
    await render('?request=abc');
    await click(button('연결하지 않을래요'));
    await confirmDialog();
    expect((h.req.mock.calls[1][1] as { body: { decision: string } }).body.decision).toBe('deny');
    expect(el.textContent).toContain('연결하지 않았어요');
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
    expect(el.textContent).toContain('열 수 없어요');
    expect(el.textContent).toContain('연결 요청 시간(10분)이 지났어요.');
  });

  it('shows a developer app asking for the API as an app, not an AI assistant', async () => {
    const app = {
      ...details, audience: 'api', requested_from: 'myapp.example', return_to: 'myapp.example',
      client: { name: 'Release Helper', client_id: 'audeniq-app-x', verified_host: null, registered: false,
        developer: true, website: 'https://www.myapp.example', description: '발매를 준비해요', private: true },
    };
    h.req.mockResolvedValueOnce(app).mockResolvedValueOnce({ kind: 'browser', status: 'APPROVED', redirect: 'https://myapp.example/cb?code=x', scopes: ['releases:read'] });
    await render('?request=abc');
    expect(el.querySelector('.wizard-top-title')?.textContent).toBe('앱 연결');
    expect(el.textContent).toContain('myapp.example · 개발자 앱');
    expect(el.textContent).not.toContain('myapp.example에서 요청');
    expect(el.textContent).toContain('내 앱');
    expect(el.textContent).toContain('나만 연결할 수 있어요');
    expect(el.textContent).toContain('발매를 준비해요');
    expect(el.textContent).not.toContain('확인하지 못했어요');
    await click(el.querySelector('#wizardNext'));
    expect(el.textContent).toContain('Release Helper로 돌아가고 있어요.');
  });

  it('asks for the code when none was given', async () => {
    await render('?device=1');
    expect(el.textContent).toContain('입력해 주세요');
    expect(h.req).not.toHaveBeenCalled();
  });
});
