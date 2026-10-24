import { describe, expect, it } from 'vitest';
import { releaseWhen } from './ArtistPage';

const r = (release_date: string | null, released: boolean, type = 'EP') => ({ slug: 'abcd2345', title: 't', type, release_date, released, cover: false });

describe('아티스트 페이지 발매 표기', () => {
  it('나온 발매는 유형 · 해, 나올 발매는 날짜', () => {
    expect(releaseWhen(r('2026-08-01', true), 2026)).toBe('EP · 2026');
    expect(releaseWhen(r('2026-11-15', false), 2026)).toBe('11월 15일 발매 예정');
    expect(releaseWhen(r('2027-01-09', false), 2026)).toBe('2027년 1월 9일 발매 예정');
    expect(releaseWhen(r(null, true, 'SINGLE'), 2026)).toBe('싱글');
  });
});
