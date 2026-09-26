import { describe, expect, it } from 'vitest';
import {
  addDays, artistIssue, artistWarning, coverIssue, expectedReleaseType, isrcValid, minReleaseDate,
  profileLinkIssue, releaseTypeIssue, rightsLineIssue, titleIssue, titleWarning, upcValid,
} from './dsp';
import { checkAudioHeader, specLabel } from './audioSpec';

describe('배급 기준', () => {
  it('발매 유형은 트랙 수·길이로 판단한다', () => {
    expect(expectedReleaseType([200])).toBe('single');
    expect(expectedReleaseType([200, 200, 200])).toBe('single');
    expect(expectedReleaseType([200, 200, 200, 200])).toBe('ep');
    expect(expectedReleaseType([700])).toBe('ep');
    expect(expectedReleaseType(Array(7).fill(180))).toBe('album');
    expect(expectedReleaseType([900, 900, 100])).toBe('album');
    expect(releaseTypeIssue('single', [200, 200, 200, 200])).toContain('EP');
    expect(releaseTypeIssue('ep', [200, 200, 200, 200])).toBe('');
    // 길이를 모르는 곡이 있으면 곡 수가 더 작은 유형을 요구할 때는 막지 않는다
    expect(releaseTypeIssue('album', [0, 0])).toBe('');
  });

  it('제목 표기 규칙', () => {
    expect(titleIssue('여름밤 (feat. 서린)', 'track')).toContain('피처링');
    expect(titleIssue('Summer ft. A', 'release')).toContain('피처링');
    expect(titleIssue('밤 🌙', 'track')).toContain('이모지');
    expect(titleIssue('노을 (Official Audio)', 'track')).toContain('홍보');
    expect(titleIssue('노을 (Inst.)', 'track')).toContain('버전');
    expect(titleIssue('노을 (Inst.)', 'release')).toBe('');
    expect(titleIssue('Without You', 'track')).toBe('');
    expect(titleWarning('HELLO WORLD')).toContain('대문자');
    expect(titleWarning('무제')).toContain('임시');
    expect(titleWarning('Hello')).toBe('');
    expect(artistIssue('서린 feat. A')).toContain('피처링');
    expect(artistWarning('A & B')).toBeTruthy();
    expect(artistWarning('서린')).toBe('');
  });

  it('식별자·권리 표기·프로필 주소', () => {
    expect(upcValid('036000291452')).toBe(true);
    expect(upcValid('036000291453')).toBe(false);
    expect(upcValid('4006381333931')).toBe(true);
    expect(isrcValid('KR-ABC-26-00001')).toBe(true);
    expect(isrcValid('KRABC2600001')).toBe(true);
    expect(isrcValid('KR-AB-26-1')).toBe(false);
    expect(rightsLineIssue('2026 서린', '2026-09-26')).toBe('');
    expect(rightsLineIssue('℗ 2026 서린', '2026-09-26')).toContain('기호');
    expect(rightsLineIssue('서린', '2026-09-26')).toContain('연도');
    expect(rightsLineIssue('2099 서린', '2026-09-26')).toContain('연도');
    expect(profileLinkIssue('spotify', 'https://open.spotify.com/artist/0OdUWJ0sBjDrqHygGUXeCF?si=x')).toBe('');
    expect(profileLinkIssue('spotify', 'https://open.spotify.com/track/0OdUWJ0sBjDrqHygGUXeCF')).not.toBe('');
    expect(profileLinkIssue('apple', 'https://music.apple.com/kr/artist/iu/409876415')).toBe('');
    expect(profileLinkIssue('melon', 'https://www.melon.com/artist/detail.htm?artistId=261143')).toBe('');
  });

  it('커버·발매일', () => {
    expect(coverIssue(3000, 3000)).toBe('');
    expect(coverIssue(3000, 2999)).toContain('정사각형');
    expect(coverIssue(1000, 1000)).toContain('3000');
    expect(addDays('2026-12-25', 10)).toBe('2027-01-04');
    expect(minReleaseDate(false, '2026-09-26')).toBe('2026-10-10');
    expect(minReleaseDate(true, '2026-09-26')).toBe('2026-09-29');
  });
});

/** 테스트용 WAV 헤더 */
function wav({ rate = 48000, bits = 24, ch = 2, format = 1, secs = 180 } = {}): ArrayBuffer {
  const byteRate = rate * ch * (bits / 8);
  const buf = new ArrayBuffer(44);
  const v = new DataView(buf);
  const str = (o: number, s: string) => [...s].forEach((c, i) => v.setUint8(o + i, c.charCodeAt(0)));
  str(0, 'RIFF'); v.setUint32(4, 36 + byteRate * secs, true); str(8, 'WAVE');
  str(12, 'fmt '); v.setUint32(16, 16, true); v.setUint16(20, format, true); v.setUint16(22, ch, true);
  v.setUint32(24, rate, true); v.setUint32(28, byteRate, true); v.setUint16(32, ch * bits / 8, true); v.setUint16(34, bits, true);
  str(36, 'data'); v.setUint32(40, byteRate * secs, true);
  return buf;
}

function flac({ rate = 44100, bits = 16, ch = 2, samples = 44100 * 200 } = {}): ArrayBuffer {
  const buf = new ArrayBuffer(42);
  const v = new DataView(buf);
  [...'fLaC'].forEach((c, i) => v.setUint8(i, c.charCodeAt(0)));
  v.setUint8(4, 0x80); v.setUint8(7, 34); // 마지막 블록, STREAMINFO 34바이트
  const o = 8 + 10;
  v.setUint8(o, rate >> 12);
  v.setUint8(o + 1, (rate >> 4) & 0xff);
  v.setUint8(o + 2, ((rate & 0x0f) << 4) | ((ch - 1) << 1) | ((bits - 1) >> 4));
  v.setUint8(o + 3, (((bits - 1) & 0x0f) << 4) | Math.floor(samples / 2 ** 32));
  v.setUint32(o + 4, samples >>> 0);
  return buf;
}

describe('음원 규격 확인', () => {
  it('WAV 규격을 읽고 기준을 확인한다', () => {
    const ok = checkAudioHeader(wav());
    expect(ok.error).toBe('');
    expect(ok.spec).toMatchObject({ container: 'WAV', sampleRate: 48000, bitDepth: 24, channels: 2 });
    expect(Math.round(ok.spec!.duration)).toBe(180);
    expect(specLabel(ok.spec!)).toBe('WAV · 24bit · 48kHz · 스테레오');
    expect(checkAudioHeader(wav({ rate: 22050 })).error).toContain('44.1kHz');
    expect(checkAudioHeader(wav({ bits: 8 })).error).toContain('16bit');
    expect(checkAudioHeader(wav({ ch: 6 })).error).toContain('2채널');
    expect(checkAudioHeader(wav({ format: 2 })).error).toContain('압축');
    expect(checkAudioHeader(wav({ ch: 1 })).warnings[0]).toContain('모노');
    expect(checkAudioHeader(wav({ secs: 20 })).warnings[0]).toContain('30초');
  });

  it('FLAC 규격을 읽고, 확장자만 바꾼 파일은 거절한다', () => {
    const f = checkAudioHeader(flac());
    expect(f.error).toBe('');
    expect(f.spec).toMatchObject({ container: 'FLAC', sampleRate: 44100, bitDepth: 16, channels: 2 });
    expect(Math.round(f.spec!.duration)).toBe(200);
    const mp3 = new Uint8Array([0x49, 0x44, 0x33, 3, 0, 0, 0, 0, 0, 0, 0xff, 0xfb]).buffer;
    expect(checkAudioHeader(mp3).error).toContain('WAV·FLAC 원본');
  });
});

describe('DSP 코드', () => {
  it('스튜디오 플랫폼 키를 서버 레지스트리 코드로 바꾼다', async () => {
    const { dspCode } = await import('./catalog');
    expect(dspCode('melon')).toBe('D-1');
    expect(dspCode('spotify')).toBe('D-5');
    expect(dspCode('qobuz')).toBe('D-11');
    expect(dspCode('nope')).toBe('');
  });
});
