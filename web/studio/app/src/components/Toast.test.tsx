import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ToastProvider, useToast } from './Toast';

let root: Root; let el: HTMLDivElement;
let toast!: ReturnType<typeof useToast>;
function Grab() { toast = useToast(); return null; }
beforeEach(async () => {
  vi.useFakeTimers();
  el = document.createElement('div'); document.body.appendChild(el); root = createRoot(el);
  await act(async () => { root.render(<ToastProvider><Grab /></ToastProvider>); });
});
afterEach(() => { act(() => root.unmount()); el.remove(); vi.useRealTimers(); });
const shown = () => [...el.querySelectorAll('.aq-toast:not(.is-leaving)')];

describe('Toast', () => {
  it('같은 문구는 쌓지 않고 ×2로 센다', async () => {
    await act(async () => { toast('저장했어요'); });
    await act(async () => { toast('저장했어요'); });
    expect(shown()).toHaveLength(1);
    expect(el.querySelector('.aq-toast-count')?.textContent).toBe('×2');
  });

  it('handle.update는 같은 자리에서 내용을 바꾼다', async () => {
    let h!: ReturnType<typeof toast>;
    await act(async () => { h = toast('올리는 중이에요', 'loading'); });
    expect(el.querySelector('.aq-toast.is-loading')).not.toBeNull();
    await act(async () => { h.update('올렸어요', 'success'); });
    expect(shown()).toHaveLength(1);
    expect(shown()[0].className).toContain('is-success');
    expect(shown()[0].textContent).toContain('올렸어요');
  });

  it('진행 중은 저절로 닫히지 않고, 결과로 바뀌면 시간을 센다', async () => {
    let h!: ReturnType<typeof toast>;
    await act(async () => { h = toast('올리는 중이에요', 'loading'); });
    await act(async () => { vi.advanceTimersByTime(20000); });
    expect(shown()).toHaveLength(1);
    await act(async () => { h.update('올렸어요', 'success'); });
    await act(async () => { vi.advanceTimersByTime(3600); });
    expect(shown()).toHaveLength(0);
  });

  it('버튼을 누르면 handle을 받아 되돌린다', async () => {
    const undo = vi.fn();
    await act(async () => { toast('크레딧을 지웠어요', { tone: 'success', action: { label: '되돌리기', onClick: h => { undo(); h.update('되돌렸어요', 'success'); } } }); });
    await act(async () => { el.querySelector<HTMLButtonElement>('.aq-toast-act')!.click(); });
    expect(undo).toHaveBeenCalledOnce();
    expect(shown()[0].textContent).toContain('되돌렸어요');
    expect(el.querySelector('.aq-toast-act')).toBeNull();
  });

  it('최대 2개만 보인다', async () => {
    for (const m of ['하나', '둘', '셋']) await act(async () => { toast(m, 'info'); });
    expect(shown().map(x => x.textContent)).toEqual(['둘', '셋']);
  });
});
