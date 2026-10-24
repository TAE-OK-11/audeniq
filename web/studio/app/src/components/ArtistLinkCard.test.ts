import { describe, expect, it } from 'vitest';
import { linkSummary } from './ArtistLinkCard';

const row = (link: { slug: string; enabled: boolean; bio: string; views: number } | null) => ({ artist_id: 'a', name: '서린', releases: 1, link });

describe('아티스트 소개 페이지 줄', () => {
  it('없으면 만들기, 꺼짐, 공개 중이면 조회 수', () => {
    expect(linkSummary(row(null))).toEqual({ text: '만들기', empty: true });
    expect(linkSummary(row({ slug: 'abcd2345', enabled: false, bio: '', views: 3 })).text).toBe('꺼짐');
    expect(linkSummary(row({ slug: 'abcd2345', enabled: true, bio: '', views: 1284 })).text).toBe('공개 중 · 최근 30일 1,284회');
  });
});
