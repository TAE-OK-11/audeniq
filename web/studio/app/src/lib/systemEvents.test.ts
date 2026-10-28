import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import * as content from '../api/content';
import { classifyServerIssue, reportServerIssue } from './systemEvents';
import { SystemStatus, kstWhen, windowLabel } from '../components/SystemStatus';

describe('classifyServerIssue', () => {
  it('게이트웨이·DB 장애는 down, MAINTENANCE는 점검, 나머지는 무시', () => {
    expect(classifyServerIssue(502, 'BACKEND_UNAVAILABLE')).toEqual({ kind: 'down', status: 502, code: 'BACKEND_UNAVAILABLE' });
    expect(classifyServerIssue(504, '')).toEqual({ kind: 'down', status: 504, code: 'HTTP_504' });
    expect(classifyServerIssue(503, 'MAINTENANCE')?.kind).toBe('maintenance');
    expect(classifyServerIssue(500, 'INTERNAL_ERROR')).toEqual({ kind: 'down', status: 500, code: 'INTERNAL_ERROR' });
    expect(classifyServerIssue(404, 'NOT_FOUND')).toBeNull();
    expect(classifyServerIssue(409, 'CONFLICT')).toBeNull();
  });
});

describe('점검 시각 표기 (한국 시간)', () => {
  it('같은 날이면 끝 시각만, 날짜가 다르면 둘 다', () => {
    expect(kstWhen('2026-09-29T17:00:00Z')).toBe('9월 30일(수) 02:00');
    expect(windowLabel({ starts_at: '2026-09-29T17:00:00Z', ends_at: '2026-09-29T19:30:00Z' })).toBe('9월 30일(수) 02:00 ~ 04:30');
    expect(windowLabel({ starts_at: '2026-09-29T14:00:00Z', ends_at: '2026-09-29T16:00:00Z' })).toBe('9월 29일(화) 23:00 ~ 9월 30일(수) 01:00');
    expect(windowLabel({ starts_at: '2026-09-29T17:00:00Z', ends_at: '9999-12-31T00:00:00Z', end_unknown: true })).toBe('9월 30일(수) 02:00부터 · 종료 시각 미정');
  });
});


const mocks = vi.hoisted(() => ({ toast: vi.fn(), pathname: '/' }));
vi.mock('../components/Toast', () => ({ useToast: () => mocks.toast }));
vi.mock('./router', async importOriginal => ({
  ...await importOriginal<typeof import('./router')>(),
  useLocation: () => ({ pathname: mocks.pathname, state: null }),
}));

describe('SystemStatus recovery', () => {
  let root: Root;
  let host: HTMLDivElement;
  const healthy = { now: '', maintenance: { active: null, upcoming: null } };
  const down = () => reportServerIssue({ kind: 'down', status: 502, code: 'UPSTREAM_UNAVAILABLE' });
  const mount = async () => {
    await act(async () => root.render(createElement(SystemStatus, null,
      createElement('input', { defaultValue: 'unsaved input' }))));
  };

  beforeEach(() => {
    vi.useFakeTimers();
    vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
    sessionStorage.setItem('aq.device-checked', '1');
    mocks.pathname = '/';
    host = document.createElement('div');
    document.body.appendChild(host);
    root = createRoot(host);
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    sessionStorage.clear();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  it('keeps a newer outage until a subsequent probe succeeds, preserving input', async () => {
    let resolve!: (value: content.ServiceStatus) => void;
    const probe = vi.spyOn(content, 'fetchStatus')
      .mockImplementationOnce(() => new Promise(r => { resolve = r; }))
      .mockResolvedValue(healthy);
    await mount();
    const input = host.querySelector('input')!;
    input.value = 'still editing';
    await act(async () => down());
    expect(document.querySelector('.aq-maint-layer')).not.toBeNull();
    expect(host.querySelector('.aq-app-shell')?.hasAttribute('inert')).toBe(true);
    await act(async () => resolve(healthy));
    expect(document.querySelector('.aq-maint-layer')).not.toBeNull();
    await act(async () => { await vi.advanceTimersByTimeAsync(30_000); });
    expect(probe).toHaveBeenCalledTimes(2);
    expect(document.querySelector('.aq-maint-layer')).toBeNull();
    expect(host.querySelector('input')).toBe(input);
    expect(input.value).toBe('still editing');
    expect(host.querySelector('.aq-app-shell')?.hasAttribute('inert')).toBe(false);
  });

  it('suppresses the device dialog during maintenance and ignores an older probe', async () => {
    sessionStorage.removeItem('aq.device-checked');
    vi.stubGlobal('isSecureContext', false);
    let resolve!: (value: content.ServiceStatus) => void;
    vi.spyOn(content, 'fetchStatus').mockImplementationOnce(() => new Promise(r => { resolve = r; }));
    await mount();
    expect(document.querySelectorAll('[role="dialog"]')).toHaveLength(1);
    await act(async () => reportServerIssue({ kind: 'maintenance', status: 503, code: 'MAINTENANCE' }));
    await act(async () => resolve(healthy));
    expect(document.querySelectorAll('[role="dialog"]')).toHaveLength(1);
    expect(document.querySelector('.aq-maint-layer')).not.toBeNull();
    expect(document.querySelector('.aq-sysdialog')).toBeNull();
  });

  it('leaves admin pages interactive during an outage', async () => {
    mocks.pathname = '/admin';
    vi.spyOn(content, 'fetchStatus').mockResolvedValue(healthy);
    await mount();
    await act(async () => down());
    expect(document.querySelector('.aq-maint-layer')).toBeNull();
    expect(host.querySelector('.aq-app-shell')?.hasAttribute('inert')).toBe(false);
  });
});
