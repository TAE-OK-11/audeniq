import { describe, expect, it } from 'vitest';
import { maskPreviewInput, previewLabel, previewSecs, settlePreviewInput } from './previewTime';

describe('미리듣기 시작 지점', () => {
  it('숫자만 쳐도 m:ss로 바뀐다', () => {
    expect(maskPreviewInput('0')).toBe('0');
    expect(maskPreviewInput('03')).toBe('03');
    expect(maskPreviewInput('031')).toBe('0:31');
    expect(maskPreviewInput('0310')).toBe('3:10');
    expect(maskPreviewInput('105')).toBe('1:05');
    expect(maskPreviewInput('3:10')).toBe('3:10');
    expect(maskPreviewInput('12345')).toBe('12:34');
  });
  it('칸을 떠나면 초만 적은 값도 m:ss로', () => {
    expect(settlePreviewInput('45')).toBe('0:45');
    expect(settlePreviewInput('90')).toBe('1:30');
    expect(settlePreviewInput('1:75')).toBe('1:75');
    expect(settlePreviewInput('')).toBe('');
  });
  it('서버에는 초로', () => {
    expect(previewSecs('3:10')).toBe('190');
    expect(previewSecs('1:75')).toBe('');
    expect(previewLabel(65)).toBe('1:05');
  });
});
