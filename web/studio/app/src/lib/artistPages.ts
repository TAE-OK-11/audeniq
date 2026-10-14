// 아티스트 플랫폼 페이지 — 한 번 기록한 스포티파이·애플뮤직·멜론 등 아티스트 페이지를 다음 신청에 다시 쓴다.
// 서버 crates/core/src/artist_pages.rs와 같은 키.
import { orgPath, req } from '../api/http';
import { MOCK } from './mode';

export const PAGE_PLATFORMS: { key: string; label: string; placeholder: string }[] = [
  { key: 'spotify', label: 'Spotify', placeholder: 'https://open.spotify.com/artist/...' },
  { key: 'apple', label: 'Apple Music', placeholder: 'https://music.apple.com/kr/artist/...' },
  { key: 'melon', label: '멜론', placeholder: 'https://www.melon.com/artist/detail.htm?artistId=...' },
  { key: 'genie', label: '지니', placeholder: 'https://www.genie.co.kr/detail/artistInfo?...' },
  { key: 'flo', label: 'FLO', placeholder: 'https://www.music-flo.com/detail/artist/...' },
  { key: 'bugs', label: '벅스', placeholder: 'https://music.bugs.co.kr/artist/...' },
  { key: 'vibe', label: 'VIBE', placeholder: 'https://vibe.naver.com/artist/...' },
  { key: 'youtube', label: 'YouTube', placeholder: 'https://www.youtube.com/channel/...' },
];

export interface PageInfo { url: string | null; status: 'PENDING' | 'LINKED'; first_release_id: string | null; source?: string }
export interface ArtistPages { artist_id: string | null; pages: Record<string, PageInfo> }

/** 아티스트명으로 기록된 페이지 (없으면 빈 값) */
export async function fetchArtistPages(name: string): Promise<ArtistPages> {
  if (MOCK || !name.trim()) return { artist_id: null, pages: {} };
  try { return await req<ArtistPages>(orgPath(`/artist-pages?name=${encodeURIComponent(name.trim())}`), { quiet401: true }); }
  catch { return { artist_id: null, pages: {} }; }
}
