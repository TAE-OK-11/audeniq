import { describe, expect, it } from 'vitest';
import { platformKey, platformLabel } from './platformNames';

describe('플랫폼 이름', () => {
  it('리포트 표기를 스튜디오 표기로', () => {
    expect(platformLabel('Melon')).toBe('멜론');
    expect(platformLabel('Apple Music')).toBe('Apple Music');
    expect(platformKey('YouTube Music')).toBe('youtube');
    expect(platformKey('youtube_music')).toBe('youtube');
    expect(platformLabel('Anghami')).toBe('Anghami');
    expect(platformKey('Anghami')).toBeUndefined();
  });
});
