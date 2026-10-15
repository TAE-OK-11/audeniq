// 스마트링크 카드 — 서버가 채운 링크를 칸에 보여 주고, 붙여 넣은 링크를 확인해 칸마다 안내하며,
// 공식 오픈 전(test)에는 다른 앨범도 저장되고 오픈 뒤(strict)에는 저장 버튼이 막히는지.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ToastProvider } from './Toast';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const http = vi.hoisted(() => ({ req: vi.fn() }));
vi.mock('../api/http', () => ({ req: http.req, orgPath: (p: string) => `/api/orgs/o1${p}` }));
vi.mock('../lib/mode', () => ({ MOCK: false }));

import { SmartLinkCard } from './SmartLinkCard';

const MELON = 'https://www.melon.com/album/detail.htm?albumId=11205713';
const status = (verify: 'test' | 'strict') => ({
  link: { slug: 'abcd2345', enabled: true, links: {} },
  auto: { melon: MELON },
  checks: {},
  verify,
  stats: { views: 0, clicks: {}, days: 30 },
  platforms: ['melon', 'spotify'],
  can_create: true,
});

let root: Root; let el: HTMLDivElement;
beforeEach(() => { vi.useFakeTimers(); el = document.createElement('div'); document.body.appendChild(el); root = createRoot(el); });
afterEach(() => { act(() => root.unmount()); el.remove(); http.req.mockReset(); vi.useRealTimers(); });

async function render(verify: 'test' | 'strict', check: string) {
  http.req.mockImplementation((url: string) => Promise.resolve(url.endsWith('/check') ? { checks: { spotify: check }, verify } : status(verify)));
  await act(async () => { root.render(<ToastProvider><SmartLinkCard releaseId="r1" /></ToastProvider>); });
  await act(async () => { [...el.querySelectorAll('button')].find(b => b.textContent === '링크 수정')!.click(); });
  const spotify = el.querySelector<HTMLInputElement>('#sl-spotify')!;
  await act(async () => {
    const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!;
    set.call(spotify, 'https://open.spotify.com/album/1111111111111111111111');
    spotify.dispatchEvent(new Event('input', { bubbles: true }));
  });
  await act(async () => { vi.advanceTimersByTime(450); });
  const save = [...el.querySelectorAll('button')].find(b => b.textContent === '저장')!;
  return { spotify, save };
}

describe('SmartLinkCard', () => {
  it('shows the server-filled link and says it is used when the field stays empty', async () => {
    await render('test', 'verified');
    expect(el.textContent).toContain('플랫폼에 공개된 1곳은 링크를 자동으로 채웠어요');
    const melon = el.querySelector<HTMLInputElement>('#sl-melon')!;
    expect(melon.value).toBe('');
    expect(melon.placeholder).toBe(MELON);
    expect(el.querySelector('#sl-melon-note')!.textContent).toContain('자동으로 채워져요');
  });

  it('checks pasted links against the release and only reports another album before launch', async () => {
    const { spotify, save } = await render('test', 'mismatch');
    expect(http.req).toHaveBeenCalledWith('/api/orgs/o1/releases/r1/smart-link/check', expect.objectContaining({ method: 'POST' }));
    expect(el.querySelector('#sl-spotify-note')!.textContent).toBe('다른 앨범이에요 · 테스트 중이라 저장은 돼요');
    expect(spotify.getAttribute('aria-invalid')).toBeNull();
    expect(save.disabled).toBe(false);
  });

  it('blocks saving another album after launch', async () => {
    const { spotify, save } = await render('strict', 'mismatch');
    expect(el.querySelector('#sl-spotify-note')!.textContent).toBe('이 발매가 아닌 다른 앨범이에요');
    expect(spotify.getAttribute('aria-invalid')).toBe('true');
    expect(save.disabled).toBe(true);
  });

  it('confirms the reported album', async () => {
    const { save } = await render('strict', 'verified');
    expect(el.querySelector('#sl-spotify-note')!.textContent).toBe('이 발매의 앨범이에요');
    expect(save.disabled).toBe(false);
  });
});
