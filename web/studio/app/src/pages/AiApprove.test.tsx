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
