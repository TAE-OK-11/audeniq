// 발매 관련 공통 상수 — 위자드, 상세, 목록이 같은 라벨을 쓰도록 한 곳에서 관리한다.

export const KINDS: [string, string][] = [
  ['single', '싱글'], ['ep', 'EP'], ['album', '정규 앨범'], ['compilation', '컴필레이션'],
];

export const LANGUAGES: [string, string][] = [
  ['ko', '한국어'], ['en', '영어'], ['ja', '일본어'], ['other', '기타'],
];

export const DSP: [string, string][] = [
  ['melon', '멜론'], ['genie', '지니'], ['flo', 'FLO'], ['bugs', '벅스'],
  ['spotify', 'Spotify'], ['apple', 'Apple Music / iTunes'], ['youtube', 'YouTube Music'],
  ['amazon', 'Amazon Music'], ['tidal', 'TIDAL'], ['deezer', 'Deezer'], ['qobuz', 'Qobuz'],
];

/** 서버 DSP 코드 — 백엔드 레지스트리(crates/core/src/dsp_registry.rs)와 같은 순서: DSP[i] = D-(i+1) */
export const dspCode = (slug: string): string => {
  const i = DSP.findIndex(d => d[0] === slug);
  return i < 0 ? '' : `D-${i + 1}`;
};

export const GENRES: [string, string][] = [
  ['', '장르를 선택해 주세요'], ['Pop', '팝'], ['K-Pop', 'K-Pop'], ['Indie Pop', '인디 팝'],
  ['Rock', '록'], ['Indie Rock', '인디 록'], ['Alternative', '얼터너티브'], ['Hip-Hop', '힙합'],
  ['R&B / Soul', 'R&B / 소울'], ['Electronic', '일렉트로닉'], ['Dance', '댄스'], ['Jazz', '재즈'],
  ['Classical', '클래식'], ['Folk', '포크'], ['Acoustic', '어쿠스틱'], ['Ballad', '발라드'],
  ['Metal', '메탈'], ['Punk', '펑크'], ['Blues', '블루스'], ['Reggae', '레게'],
  ['Latin', '라틴'], ['World', '월드'], ['New Age', '뉴에이지'], ['OST / Soundtrack', 'OST / 사운드트랙'],
  ['Children', '어린이 음악'], ['Religious', '종교음악'], ['Ambient', '앰비언트'],
  ['Instrumental', '연주곡'], ['Spoken Word', '낭독 / 스포큰 워드'], ['__other__', '기타 · 직접 입력'],
];

export const kindLabel = (v?: string) => KINDS.find(k => k[0] === v)?.[1] || v || '';
export const dspLabel = (v: string) => DSP.find(d => d[0] === v)?.[1] || v;
export const languageLabel = (v?: string) => LANGUAGES.find(l => l[0] === v)?.[1] || v || '';
export const genreLabel = (v?: string) => {
  if (!v) return '';
  const hit = GENRES.find(g => g[0] === v);
  return hit && hit[0] !== '__other__' ? hit[1] : v;
};

const COVER_GRADIENTS = [
  'linear-gradient(135deg,#bcd8ff,#7162db 65%,#394b83)',
  'linear-gradient(135deg,#ffd9c1,#e58a7a 65%,#8a3d4b)',
  'linear-gradient(135deg,#bdf0d9,#4dae8a 65%,#2a6b52)',
  'linear-gradient(135deg,#ecd9ff,#a97ae5 65%,#5b3d8a)',
  'linear-gradient(135deg,#fff0b8,#e0a83f 65%,#8a6a2a)',
  'linear-gradient(135deg,#b8e6ff,#5c9ae5 65%,#2a4d8a)',
  'linear-gradient(135deg,#ffcfe3,#e57aa5 65%,#8a3d63)',
];

/** ID 해시로 커버 그라디언트를 고정 선택 (같은 발매는 항상 같은 색) */
export function gradientFor(id: string): string {
  let h = 0;
  for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) >>> 0;
  return COVER_GRADIENTS[h % COVER_GRADIENTS.length];
}

/** 초 → 'mm:ss' */
export function durationLabel(ms: number | null | undefined): string {
  if (!ms) return '';
  const secs = Math.round(ms / 1000);
  return `${String(Math.floor(secs / 60)).padStart(2, '0')}:${String(secs % 60).padStart(2, '0')}`;
}
