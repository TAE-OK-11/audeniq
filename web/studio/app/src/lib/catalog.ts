// 발매 관련 공통 상수 — 위자드, 상세, 목록이 같은 라벨을 쓰도록 한 곳에서 관리한다.

export const KINDS: [string, string][] = [
  ['single', '싱글'], ['ep', 'EP'], ['album', '정규 앨범'], ['compilation', '컴필레이션'],
];

export const LANGUAGES: [string, string][] = [
  ['ko', '한국어'], ['en', '영어'], ['ja', '일본어'], ['other', '기타'],
];

export const DSP: [string, string][] = [
  ['melon', '멜론'], ['genie', '지니'], ['flo', 'FLO'], ['bugs', '벅스'],
  ['spotify', 'Spotify'], ['apple', 'Apple Music'], ['youtube', 'YouTube Music'],
  ['amazon', 'Amazon Music'], ['tidal', 'TIDAL'], ['deezer', 'Deezer'], ['qobuz', 'Qobuz'],
  ['pandora', 'Pandora (SiriusXM)'], ['soundcloud', 'SoundCloud'], ['audiomack', 'Audiomack'],
  ['anghami', 'Anghami (중동·북아프리카)'], ['boomplay', 'Boomplay (아프리카)'], ['jiosaavn', 'JioSaavn (인도)'],
  ['kkbox', 'KKBOX (대만·홍콩)'], ['line-music', 'LINE MUSIC (일본)'], ['awa', 'AWA (일본)'],
  ['netease', 'NetEase Cloud Music (중국)'], ['tencent', 'Tencent Music (QQ뮤직·쿠거우·쿠워·WeSing)'],
  ['napster', 'Napster'], ['iheart', 'iHeartRadio'],
  ['meta', 'Instagram·Facebook (Meta)'], ['tiktok', 'TikTok·CapCut (ByteDance)'],
  ['youtube-cid', 'YouTube Content ID·Shorts'], ['snapchat', 'Snapchat'], ['beatport', 'Beatport'],
  ['itunes', 'iTunes Store (다운로드)'], ['claro-musica', 'Claro Música (중남미)'], ['pretzel', 'Pretzel (스트리머용 음원)'],
  ['triller', 'Triller'], ['touchtunes', 'TouchTunes (디지털 주크박스)'], ['yandex', 'Yandex Music (러시아·CIS)'],
  ['kuaishou', 'Kuaishou·Kwai·SnackVideo'], ['joox', 'JOOX (동남아·홍콩)'], ['trebel', 'TREBEL (광고형 무료 다운로드)'],
  ['mixcloud', 'Mixcloud (DJ 믹스)'], ['twitch', 'Twitch (DJ 방송)'], ['peloton', 'Peloton (피트니스)'],
  ['canva', 'Canva (디자인·영상 음원)'], ['lickd', 'Lickd (크리에이터 음원 라이선스)'], ['adaptr', 'Adaptr (앱·서비스용 음원)'],
  ['styngr', 'STYNGR (게임 음원)'],
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
/** 공식 오픈 전 테스트용 플랫폼 (서버의 MockDSP, 실제 DSP로는 아무것도 나가지 않음) */
export const MOCK_DSP: readonly [string, string] = ['mockdsp-studio', 'MockDSP (테스트)'];
export const dspLabel = (v: string) => {
  if (v === MOCK_DSP[0]) return MOCK_DSP[1];
  // 운영팀이 정산 리포트를 넣을 때 적은 값이라 대소문자·표기가 섞여 온다 (SPOTIFY, Melon)
  const norm = (s: string) => s.toLowerCase().replace(/[^a-z0-9]/g, '');
  const bySlug = DSP.find(d => d[0] === v) ?? DSP.find(d => norm(d[0]) === norm(v) || norm(d[1]) === norm(v));
  if (bySlug) return bySlug[1];
  const code = /^D-(\d+)$/i.exec(v);
  return code ? DSP[Number(code[1]) - 1]?.[1] ?? '기타 플랫폼' : '기타 플랫폼';
};
export const languageLabel = (v?: string) => LANGUAGES.find(l => l[0] === v)?.[1] || v || '';
export const genreLabel = (v?: string) => {
  if (!v) return '';
  const hit = GENRES.find(g => g[0] === v);
  return hit && hit[0] !== '__other__' ? hit[1] : v;
};

/** 발매 커버 그라데이션 — 디자인 시스템(ui/cover)과 같은 값 */
export { gradientFor } from '../ui/cover';

/** 초 → 'mm:ss' */
export function durationLabel(ms: number | null | undefined): string {
  if (!ms) return '';
  const secs = Math.round(ms / 1000);
  return `${String(Math.floor(secs / 60)).padStart(2, '0')}:${String(secs % 60).padStart(2, '0')}`;
}
