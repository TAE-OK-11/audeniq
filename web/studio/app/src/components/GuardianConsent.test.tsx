// 법정대리인 동의 — 서명은 법정대리인이 직접 하고(이 기기에서 또는 링크로), 서명과 가족관계증명서가
// 모두 준비돼야 '동의 확인 완료'를 누를 수 있는지.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ToastProvider } from './Toast';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const http = vi.hoisted(() => ({ req: vi.fn() }));
vi.mock('../api/http', () => ({ req: (...a: [string, unknown?]) => http.req(...a), orgPath: (p: string) => `/api/orgs/o1${p}`, orgId: () => 'o1' }));

import { GuardianConsentPanel } from './GuardianConsent';

const base = { release_id: 'r1', guardian: { name: '김보호', relation: '모' }, requests: [], relation_document: null, ready: false };
const signed = {
  ...base,
  requests: [{ id: 's1', status: 'SIGNED', document_kind: 'guardian' }],
  relation_document: { id: 'd1', status: 'AWAITING_DOCUMENTS', has_file: false },
};

let root: Root; let el: HTMLDivElement;
beforeEach(() => { el = document.createElement('div'); document.body.appendChild(el); root = createRoot(el); });
afterEach(() => { act(() => root.unmount()); el.remove(); http.req.mockReset(); document.body.innerHTML = ''; });

async function render(onReady = vi.fn()) {
  await act(async () => {
    root.render(<ToastProvider><GuardianConsentPanel org="o1" releaseId="r1" onClose={() => {}} onReady={onReady} /></ToastProvider>);
  });
  return onReady;
}
const button = (label: string) => [...document.querySelectorAll('button')].find(b => b.textContent === label);

describe('GuardianConsentPanel', () => {
  it('asks the named guardian to sign, on this device or by a link', async () => {
    http.req.mockImplementation((_url: string, o?: { method?: string; body?: unknown }) =>
      Promise.resolve(o?.method === 'POST' ? { id: 's1', token: 'a'.repeat(64), status: 'PENDING', expires_at: '' } : base));
    await render();
    expect(document.body.textContent).toContain('김보호(모)');
    expect(document.body.textContent).toContain('아직 서명 전이에요.');
    expect(button('동의 확인 완료')?.disabled).toBe(true);
    await act(async () => { button('법정대리인에게 링크 보내기')?.click(); });
    expect(http.req).toHaveBeenCalledWith('/api/orgs/o1/releases/r1/guardian-consent', {
      method: 'POST', body: { guardian_name: '김보호', relation: '모', channel: 'LINK' },
    });
    expect(document.body.textContent).toContain(`/sign/${'a'.repeat(64)}`);
  });

  it('points to the certificate upload once signed, and finishes only when ready', async () => {
    http.req.mockResolvedValue(signed);
    let onReady = await render();
    expect(document.body.textContent).toContain('서명을 마쳤어요.');
    expect(button('이 기기에서 서명하기')).toBeUndefined();
    const upload = [...document.querySelectorAll('a')].find(a => a.textContent === '권리·보완 서류에서 올리기');
    expect(upload?.getAttribute('href')).toBe('/rights?doc=d1');
    expect(button('동의 확인 완료')?.disabled).toBe(true);
    act(() => root.unmount()); root = createRoot(el);
    http.req.mockResolvedValue({ ...signed, relation_document: { id: 'd1', status: 'REVIEW', has_file: true }, ready: true });
    onReady = await render();
    expect(document.body.textContent).toContain('올렸어요. 심사 때 관계를 확인해요.');
    await act(async () => { button('동의 확인 완료')?.click(); });
    expect(onReady).toHaveBeenCalled();
  });
});
