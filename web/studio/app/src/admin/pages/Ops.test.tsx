// 운영 도구 — 레벨에 맞는 것만 보이고, 위험한 작업은 확인 뒤 인증 코드를 받아 보내는지.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ToastProvider } from '../../components/Toast';
import { ConfirmProvider } from '../../components/Confirm';
import { ApiError } from '../../api/errors';
import { StaffContext } from '../ui';
import type { StaffMe } from '../api';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const h = vi.hoisted(() => ({ req: vi.fn() }));
vi.mock('../../lib/mode', () => ({ MOCK: false }));
vi.mock('../../api/http', () => ({ req: (...a: [string, unknown?]) => h.req(...a), orgId: () => 'o1', orgPath: (p: string) => p }));

import { OpsFinance, OpsStaff } from './Ops';

const OPS = [
  ['staff', 'list', 3, false, false], ['staff', 'grant', 4, true, true], ['staff', 'revoke', 4, true, true], ['staff', 'reset-mfa', 4, true, true],
  ['payouts', 'list', 2, false, false], ['payouts', 'approve', 2, true, true],
] as const;
const index = (level: number, available = true, finance = false) => ({
  available, role: 'X', level,
  ops: OPS.map(([area, op, need, write, step_up]) => ({ area, op, level: need, write, step_up, finance: area === 'payouts', allowed: area === 'payouts' ? finance : level >= need })),
});
const STAFF = [
  { email: 'me@audeniq.com', user_id: 'me', role: 'OWNER', status: 'ACTIVE', granted_by: 'cli', granted_at: new Date().toISOString(), revoked_at: null },
  { email: 'cs@audeniq.com', user_id: 'cs', role: 'SUPPORT', status: 'ACTIVE', granted_by: 'cli', granted_at: new Date().toISOString(), revoked_at: null },
];

let root: Root; let el: HTMLDivElement;
beforeEach(() => { el = document.createElement('div'); document.body.appendChild(el); root = createRoot(el); });
afterEach(() => { act(() => root.unmount()); el.remove(); document.body.innerHTML = ''; h.req.mockReset(); });

async function render(me: StaffMe, page: 'staff' | 'finance' = 'staff') {
  const ctx = { me, counts: null, work: null, refreshCounts: () => {}, can: () => true };
  await act(async () => {
    root.render(<ToastProvider><ConfirmProvider><StaffContext.Provider value={ctx}>{page === 'staff' ? <OpsStaff /> : <OpsFinance />}</StaffContext.Provider></ConfirmProvider></ToastProvider>);
  });
  await settle();
}
async function settle() {
  for (let i = 0; i < 5; i += 1) await act(async () => { await new Promise(r => setTimeout(r, 0)); });
}
const buttons = (text: string) => [...document.querySelectorAll('button')].filter(b => b.textContent?.trim() === text);
const click = async (n: Element | undefined) => { await act(async () => { (n as HTMLElement).click(); }); await settle(); };
const type = async (sel: string, value: string) => {
  const input = document.querySelector(sel) as HTMLInputElement;
  const setter = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(input), 'value')!.set!;
  await act(async () => { setter.call(input, value); input.dispatchEvent(new Event('input', { bubbles: true })); });
};

describe('운영 도구', () => {
  it('관리자(레벨 3)는 직원을 보기만 하고, 등록·해제는 최고 관리자만 해요', async () => {
    h.req.mockImplementation((path: string) => Promise.resolve(path === '/api/staff/ops' ? index(3) : STAFF));
    await render({ user_id: 'admin', role: 'ADMIN', level: 3, duties: [] });
    expect(el.textContent).toContain('cs@audeniq.com');
    expect(el.textContent).not.toContain('직원 등록·레벨 바꾸기');
    expect(buttons('해제')).toHaveLength(0);
  });

  it('최고 관리자가 등록하면 확인한 뒤 인증 코드를 붙여 보내고, 틀린 코드는 다시 물어요', async () => {
    let writes = 0;
    h.req.mockImplementation((path: string, opts?: { method?: string; body?: Record<string, unknown> }) => {
      if (path === '/api/staff/ops') return Promise.resolve(index(4));
      if (opts?.method === 'POST') {
        writes += 1;
        if (writes === 1) return Promise.reject(new ApiError('코드가 틀렸어요', 400, 'STAFF_MFA_CODE_INVALID'));
        return Promise.resolve({ user_id: 'new', role: 'REVIEWER', previous: null });
      }
      return Promise.resolve(STAFF);
    });
    await render({ user_id: 'me', role: 'OWNER', level: 4, duties: [] });
    // 내 줄과 다른 최고 관리자 줄에는 관리 버튼이 없어요
    expect(buttons('해제')).toHaveLength(1);
    await type('#ops-staff-email', 'new@audeniq.com');
    await act(async () => {
      const s = document.querySelector('#ops-staff-role') as HTMLSelectElement;
      s.value = 'REVIEWER'; s.dispatchEvent(new Event('change', { bubbles: true }));
    });
    await click(buttons('등록')[0]);
    expect(document.body.textContent).toContain('직원으로 등록할까요?');
    await click(buttons('등록').at(-1));
    await type('#ops-code', '111111');
    await click(buttons('확인').at(-1));
    expect(document.body.textContent).toContain('코드가 맞지 않아요');
    await type('#ops-code', '222222');
    await click(buttons('확인').at(-1));
    const posts = h.req.mock.calls.filter(c => (c[1] as { method?: string } | undefined)?.method === 'POST');
    expect(posts).toHaveLength(2);
    expect(posts[1][0]).toBe('/api/staff/ops/staff/grant');
    expect(posts[1][1]).toMatchObject({ body: { email: 'new@audeniq.com', role: 'REVIEWER', code: '222222' } });
  });

  it('서버에서 운영 도구가 꺼져 있으면 CLI로 안내해요', async () => {
    h.req.mockImplementation(() => Promise.resolve(index(4, false)));
    await render({ user_id: 'me', role: 'OWNER', level: 4, duties: [] });
    expect(el.textContent).toContain('audeniq-admin');
    expect(el.textContent).not.toContain('직원 등록·레벨 바꾸기');
  });

  it('정산 업무가 없으면 지급 처리를 볼 수 없어요', async () => {
    h.req.mockImplementation(() => Promise.resolve(index(2, true, false)));
    await render({ user_id: 'r', role: 'REVIEWER', level: 2, duties: ['REVIEW'] }, 'finance');
    expect(el.textContent).toContain('정산 업무(배급 운영·관리자·최고 관리자)만');
  });
});
