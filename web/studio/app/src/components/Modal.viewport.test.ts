// iOS 키보드: 보이는 영역이 줄면 키보드에 가린 높이·보이는 높이를 달고, 키보드가 내려가면 지우는지.
// (어두운 배경 크기는 CSS가 넉넉히 덮으므로 여기서 바꾸지 않는다)
import { describe, expect, it } from 'vitest';
import { fitToVisualViewport } from './Modal';

const vv = (height: number, offsetTop: number) => ({ height, offsetTop }) as VisualViewport;

describe('fitToVisualViewport', () => {
  it('sets the keyboard height and the visible height while the keyboard is up', () => {
    const el = document.createElement('div');
    fitToVisualViewport(el, vv(420, 180), 844);
    expect(el.style.getPropertyValue('--aq-kb')).toBe('244px');
    expect(el.style.getPropertyValue('--aq-vv-h')).toBe('420px');
    expect(el.style.getPropertyValue('top')).toBe('');
    expect(el.style.getPropertyValue('height')).toBe('');
  });

  it('clears them when the keyboard closes', () => {
    const el = document.createElement('div');
    fitToVisualViewport(el, vv(420, 180), 844);
    fitToVisualViewport(el, vv(844, 0), 844);
    expect(el.style.getPropertyValue('--aq-kb')).toBe('');
    expect(el.style.getPropertyValue('--aq-vv-h')).toBe('');
  });
});

describe('coverOutsideFixedLayer', () => {
  it('lays a white strip from the sheet bottom and a dim strip above the screen, and removes them', async () => {
    const { coverOutsideFixedLayer } = await import('./Modal');
    const overlay = document.createElement('div');
    overlay.style.backgroundColor = 'rgba(19, 36, 66, 0.5)';
    const sheet = document.createElement('section');
    document.body.append(overlay, sheet);
    sheet.getBoundingClientRect = () => ({ top: 500, bottom: 800, left: 0, right: 390, width: 390, height: 300, x: 0, y: 500, toJSON: () => ({}) });
    const stop = coverOutsideFixedLayer(overlay, sheet);
    const covers = [...document.querySelectorAll<HTMLElement>('[data-aq-sheet-cover]')];
    expect(covers).toHaveLength(2);
    expect(covers[0].style.top).toBe(`${window.scrollY + 800 - 40}px`);
    expect(covers[0].style.background).toContain('255, 255, 255');
    expect(covers[1].style.background).toBe('rgb(137, 146, 161)');
    stop();
    expect(document.querySelectorAll('[data-aq-sheet-cover]')).toHaveLength(0);
    overlay.remove(); sheet.remove();
  });
});
