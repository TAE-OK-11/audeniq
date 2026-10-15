// iOS 키보드: 배경을 보이는 영역(visual viewport)에 맞추고, 키보드가 내려가면 원래대로 돌려놓는지.
import { describe, expect, it } from 'vitest';
import { fitToVisualViewport } from './Modal';

const vv = (height: number, offsetTop: number) => ({ height, offsetTop }) as VisualViewport;

describe('fitToVisualViewport', () => {
  it('follows the visible area while the keyboard is up', () => {
    const el = document.createElement('div');
    fitToVisualViewport(el, vv(420, 180), 844);
    expect(el.style.getPropertyValue('top')).toBe('180px');
    expect(el.style.getPropertyValue('height')).toBe('1264px'); // 보이는 영역 + 아래로 넉넉히
    expect(el.style.getPropertyValue('bottom')).toBe('auto');
    expect(el.style.getPropertyPriority('top')).toBe('important');
    expect(el.style.getPropertyValue('--aq-vv-h')).toBe('420px');
    expect(el.style.getPropertyValue('--aq-kb')).toBe('244px');
  });

  it('goes back to the full screen when the keyboard closes', () => {
    const el = document.createElement('div');
    fitToVisualViewport(el, vv(420, 180), 844);
    fitToVisualViewport(el, vv(844, 0), 844);
    expect(el.style.getPropertyValue('top')).toBe('');
    expect(el.style.getPropertyValue('height')).toBe('');
    expect(el.style.getPropertyValue('bottom')).toBe('');
    expect(el.style.getPropertyValue('--aq-vv-h')).toBe('');
    expect(el.style.getPropertyValue('--aq-kb')).toBe('');
  });
});
