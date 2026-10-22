// AI 앱이 준 승인 링크가 안 열릴 때 — "없음"만 보여 주지 않고, 흔한 이유와 다음 할 일을 보여 주는지.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ToastProvider } from '../components/Toast';
import { ConfirmProvider } from '../components/Confirm';
import { ApiError } from '../api/errors';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const http = vi.hoisted(() => ({ req: vi.fn() }));
vi.mock('../api/http', () => ({ req: (...args: [string, unknown?]) => http.req(...args), orgId: () => 'o1', orgPath: (p: string) => `/api/orgs/o1${p}` }));
vi.mock('../lib/mode', () => ({ MOCK: false }));
vi.mock('../api/auth', () => ({ useAuth: () => ({ user: { email: 'hana@example.com' } }) }));
vi.mock('../lib/router', () => ({
  Link: ({ to, children, ...rest }: { to: string; children: unknown }) => <a href={to} {...rest}>{children as never}</a>,
  useParams: () => ({ id: 'q1' }),
}));

import { AiApprove } from './AiApprove';
import { AiAction } from './AiAction';

let root: Root; let el: HTMLDivElement;
beforeEach(() => { el = document.createElement('div'); document.body.appendChild(el); root = createRoot(el); });
afterEach(() => { act(() => root.unmount()); el.remove(); http.req.mockReset(); });

async function render(node: React.ReactNode) {
  await act(async () => {
    root.render(<ToastProvider><ConfirmProvider>{node}</ConfirmProvider></ToastProvider>);
  });
}

describe('AI approval links that do not open', () => {
  it('explains why a permission request is missing and what to do next', async () => {
    http.req.mockRejectedValue(new ApiError('요청한 정보를 찾을 수 없어요.', 404, 'NOT_FOUND'));
    await render(<AiApprove />);
    expect(el.textContent).toContain('요청을 찾지 못했어요');
    expect(el.textContent).toContain('다른 계정으로 로그인했어요');
    expect(el.textContent).toContain('60분');
    expect(el.textContent).not.toContain('요청한 정보를 찾을 수 없어요');
    expect(el.querySelector('a[href="/profile?open=ai"]')).not.toBeNull();
  });

  it('uses the 24-hour window for action requests', async () => {
    http.req.mockRejectedValue(new ApiError('요청한 정보를 찾을 수 없어요.', 404, 'NOT_FOUND'));
    await render(<AiAction />);
    expect(el.textContent).toContain('24시간');
  });

  it('keeps other errors as errors', async () => {
    http.req.mockRejectedValue(new ApiError('서버에 연결하지 못했어요.', 503, 'UNAVAILABLE'));
    await render(<AiApprove />);
    expect(el.textContent).toContain('서버에 연결하지 못했어요');
    expect(el.textContent).not.toContain('요청을 찾지 못했어요');
  });
});

const pending = (over: Record<string, unknown> = {}) => ({
  id: 'q1', status: 'PENDING', scopes: ['releases:submit'], reason: '', client_name: 'Claude', org_id: 'o1', org_name: 'Personal workspace',
  created_at: new Date().toISOString(), expires_at: new Date(Date.now() + 47 * 60_000 + 30_000).toISOString(), decided_at: null,
  connection_scopes: ['releases:read'], connection_label: null, connection_revoked: false, ...over,
});

describe('AI approval pages while open', () => {
  it('counts down the time left', async () => {
    http.req.mockResolvedValue(pending());
    await render(<AiApprove />);
    expect(el.textContent).toContain('47분 남았어요');
  });

  it('shows an expired request as expired before any button is pressed', async () => {
    http.req.mockResolvedValue(pending({ expires_at: new Date(Date.now() - 1000).toISOString() }));
    await render(<AiApprove />);
    expect(el.textContent).toContain('답할 수 있는 시간이 지났어요');
    expect(el.querySelector('#wizardNext')?.textContent).not.toBe('권한 주기');
  });

  it('offers the next waiting request after one is answered', async () => {
    http.req.mockImplementation(async (path: string) => {
      if (path.endsWith('/ai-permission-requests')) return { items: [pending({ id: 'q2' })] };
      if (path.endsWith('/ai-action-requests')) return { items: [] };
      return pending({ status: 'APPROVED' });
    });
    await render(<AiApprove />);
    expect(el.textContent).toContain('답을 기다리는 요청이 1개 더 있어요');
    expect(el.querySelector('a[href="/ai/approve/q2"]')).not.toBeNull();
  });

  it('lets a failed load be tried again', async () => {
    http.req.mockRejectedValueOnce(new ApiError('서버에 연결하지 못했어요.', 503, 'UNAVAILABLE')).mockResolvedValue(pending());
    await render(<AiApprove />);
    const retry = [...el.querySelectorAll('button')].find(b => b.textContent === '다시 시도');
    expect(retry).toBeDefined();
    await act(async () => { retry!.click(); });
    expect(el.textContent).toContain('권한을 더 줄까요?');
  });
});
