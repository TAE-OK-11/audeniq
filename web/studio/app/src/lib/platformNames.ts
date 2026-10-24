// 스마트링크 플랫폼 표시 이름 (서버 crates/core/src/smart_links.rs PLATFORMS 순서와 같은 키)
export const PLATFORM_NAMES: Record<string, string> = {
  melon: '멜론', genie: '지니', flo: 'FLO', bugs: '벅스', vibe: 'VIBE', spotify: 'Spotify', apple: 'Apple Music',
  youtube: 'YouTube Music', amazon: 'Amazon Music', deezer: 'Deezer', tidal: 'TIDAL',
};

/** 리포트·배급에 오는 플랫폼 이름('Melon', 'Apple Music', 'youtube_music' 등) → 위 목록의 키. 모르는 곳은 undefined */
const ALIAS: Record<string, string> = {
  melon: 'melon', 멜론: 'melon', genie: 'genie', 지니: 'genie', geniemusic: 'genie', flo: 'flo', bugs: 'bugs', 벅스: 'bugs',
  vibe: 'vibe', navervibe: 'vibe', spotify: 'spotify', apple: 'apple', applemusic: 'apple', itunes: 'apple',
  youtube: 'youtube', youtubemusic: 'youtube', amazon: 'amazon', amazonmusic: 'amazon', deezer: 'deezer', tidal: 'tidal',
};
export function platformKey(name: string): string | undefined {
  return ALIAS[name.toLowerCase().replace(/[\s_.-]/g, '')];
}
/** 화면에 보일 이름 — 아는 곳은 스튜디오 표기(멜론·지니…)로, 모르는 곳은 받은 그대로 */
export function platformLabel(name: string): string {
  const k = platformKey(name);
  return k ? PLATFORM_NAMES[k] : name;
}
