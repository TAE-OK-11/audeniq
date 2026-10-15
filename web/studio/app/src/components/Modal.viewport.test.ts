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
