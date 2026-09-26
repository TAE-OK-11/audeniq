// 실제 배급(스트리밍 플랫폼 납품) 기준 — 플랫폼 스타일 가이드와 국내외 유통사 공통 요건을 신청 단계에서 미리 확인한다.
import { todayStr } from './date';

// ---------------------------------------------------------------------------
// 발매일
// ---------------------------------------------------------------------------
/** 플랫폼 납품·검수에 필요한 최소 준비 기간 (일) */
export const LEAD_DAYS = { normal: 14, express: 3 } as const;

export function addDays(iso: string, days: number): string {
  const [y, m, d] = iso.split('-').map(Number);
  return todayStr(new Date(y, m - 1, d + days));
}

export function minReleaseDate(express: boolean, today = todayStr()): string {
  return addDays(today, express ? LEAD_DAYS.express : LEAD_DAYS.normal);
}

// ---------------------------------------------------------------------------
// 발매 유형 — 트랙 수·길이 기준 (Spotify·Apple Music 공통 규칙)
//   싱글: 1–3곡, 각 10분 미만, 합계 30분 미만
//   EP:   1–3곡 중 10분 이상 곡이 있거나 4–6곡, 합계 30분 미만
//   앨범: 7곡 이상 또는 합계 30분 이상
// ---------------------------------------------------------------------------
export function expectedReleaseType(durations: number[]): 'single' | 'ep' | 'album' {
  const n = durations.length;
  const total = durations.reduce((a, b) => a + b, 0);
  const long = durations.some(d => d >= 600);
  if (n >= 7 || total >= 1800) return 'album';
  if (n >= 4 || long) return 'ep';
  return 'single';
}

const TYPE_NAME: Record<string, string> = { single: '싱글', ep: 'EP', album: '정규 앨범' };

/** 선택한 유형이 트랙 구성과 맞지 않으면 안내 문구 (컴필레이션은 곡 수만 확인) */
export function releaseTypeIssue(type: string, durations: number[]): string {
  if (!durations.length) return '';
  if (type === 'compilation') return durations.length < 2 ? '컴필레이션은 2곡 이상이어야 해요.' : '';
  const expected = expectedReleaseType(durations);
  if (expected === type) return '';
  // 길이를 모르는 곡이 있으면 곡 수로만 판단한 결과가 더 큰 유형일 때만 막는다
  const unknown = durations.some(d => !d);
  if (unknown && ['single', 'ep', 'album'].indexOf(expected) < ['single', 'ep', 'album'].indexOf(type)) return '';
  return `${durations.length}곡${unknown ? '' : `, 총 ${Math.round(durations.reduce((a, b) => a + b, 0) / 60)}분`} 구성은 플랫폼 기준으로 ‘${TYPE_NAME[expected]}’이에요. 발매 정보에서 발매 유형을 ${TYPE_NAME[expected]}(으)로 바꿔 주세요.`;
}

// ---------------------------------------------------------------------------
// 제목·이름 표기 (플랫폼 스타일 가이드)
// ---------------------------------------------------------------------------
const EMOJI = /\p{Extended_Pictographic}/u;
const FEAT = /(^|[\s(\[])(feat\.?|ft\.?|featuring|with)\s/i;
const PROMO = /[([]\s*(official|m\/?v|music video|lyric video|visualizer|audio|free download|explicit|clean|new)\b[^)\]]*[)\]]/i;
const VERSION_IN_TITLE = /[([]\s*(inst\.?|instrumental|remix|acoustic|live|remaster(ed)?|sped up|slowed|demo|radio edit|extended|ver\.?|version|반주|리믹스|라이브)[^)\]]*[)\]]/i;
const URLISH = /(https?:\/\/|www\.|\.com\b|@\w)/i;

/** 발매·곡 제목 오류 (배급 거절 사유) */
export function titleIssue(title: string, kind: 'release' | 'track'): string {
  const t = title.trim();
  if (!t) return '';
  if (EMOJI.test(t)) return '제목에는 이모지를 쓸 수 없어요.';
  if (URLISH.test(t)) return '제목에 주소·SNS 계정을 넣을 수 없어요.';
  if (FEAT.test(t)) return kind === 'track'
    ? '피처링은 제목에 쓰지 말고 ‘피처링 아티스트’ 칸에 입력해 주세요.'
    : '피처링은 제목에 쓰지 말고 곡의 ‘피처링 아티스트’ 칸에 입력해 주세요.';
  if (PROMO.test(t)) return '(Official Audio)·(MV) 같은 홍보 문구는 제목에 넣을 수 없어요.';
  if (kind === 'track' && VERSION_IN_TITLE.test(t)) return '(Inst.)·(Remix) 같은 버전 표기는 제목 대신 ‘버전 / 부제’ 칸에 입력해 주세요.';
  return '';
}

/** 아티스트명 오류 */
export function artistIssue(name: string): string {
  const t = name.trim();
  if (!t) return '';
  if (EMOJI.test(t)) return '아티스트명에는 이모지를 쓸 수 없어요.';
  if (FEAT.test(` ${t} `)) return '피처링 아티스트는 곡의 ‘피처링 아티스트’ 칸에 따로 입력해 주세요.';
  return '';
}

/** 여러 아티스트를 한 칸에 쓴 것처럼 보이면 (팀 이름일 수도 있어 경고만) */
export function artistWarning(name: string): string {
  return /\s(&|x|×|and)\s|,/i.test(name.trim())
    ? '여러 아티스트를 한 칸에 쓴 것처럼 보여요. 팀 이름이 아니라면 대표 아티스트만 쓰고 참여 아티스트는 곡의 피처링 칸에 입력해 주세요.'
    : '';
}

/** 배급은 되지만 검수에서 수정 요청이 올 수 있는 표기 */
export function titleWarning(title: string): string {
  const t = title.trim();
  const latin = t.replace(/[^A-Za-z]/g, '');
  if (latin.length >= 4 && latin === latin.toUpperCase() && t.length > 3) return '영문을 모두 대문자로 쓰면 플랫폼 검수에서 수정될 수 있어요.';
  if (/^(track|untitled|무제|제목 ?없음|test|테스트)\s*\d*$/i.test(t)) return '임시 제목처럼 보여요. 실제 발매 제목인지 확인해 주세요.';
  if (/\s{2,}/.test(title)) return '공백이 두 번 이상 들어가 있어요.';
  return '';
}

// ---------------------------------------------------------------------------
// 식별자
// ---------------------------------------------------------------------------
/** UPC(12자리)·EAN(13자리) 체크 숫자 검증 */
export function upcValid(code: string): boolean {
  if (!/^\d{12,13}$/.test(code)) return false;
  const digits = code.split('').map(Number);
  const check = digits.pop()!;
  const sum = digits.reverse().reduce((s, d, i) => s + d * (i % 2 === 0 ? 3 : 1), 0);
  return (10 - (sum % 10)) % 10 === check;
}

export const normalizeIsrc = (v: string) => v.replace(/[-\s]/g, '').toUpperCase();
export const isrcValid = (v: string) => /^[A-Z]{2}[A-Z0-9]{3}\d{2}\d{5}$/.test(normalizeIsrc(v));

// ---------------------------------------------------------------------------
// 권리 표기 — "2026 권리자명" (기호는 플랫폼이 붙인다)
// ---------------------------------------------------------------------------
export function rightsLineIssue(line: string, today = todayStr()): string {
  const t = line.trim();
  if (!t) return '';
  if (/[℗©]|\((c|p)\)/i.test(t)) return '℗·© 기호는 빼고 ‘연도 권리자명’으로 입력해 주세요.';
  const m = /^(\d{4})\s+\S/.exec(t);
  if (!m) return '‘2026 권리자명’처럼 연도와 권리자명을 함께 입력해 주세요.';
  const y = +m[1];
  if (y < 1900 || y > +today.slice(0, 4) + 1) return '권리 표기의 연도를 확인해 주세요.';
  return '';
}

// ---------------------------------------------------------------------------
// 기존 아티스트 프로필 연결 (동명이인 페이지로 잘못 올라가는 것을 막는다)
// ---------------------------------------------------------------------------
export const PROFILE_LINKS: { key: 'spotify' | 'apple' | 'melon'; label: string; placeholder: string; re: RegExp }[] = [
  { key: 'spotify', label: 'Spotify', placeholder: 'https://open.spotify.com/artist/…', re: /^(https:\/\/open\.spotify\.com\/(intl-[a-z-]+\/)?artist\/[A-Za-z0-9]{22}|spotify:artist:[A-Za-z0-9]{22})/ },
  { key: 'apple', label: 'Apple Music', placeholder: 'https://music.apple.com/kr/artist/…', re: /^https:\/\/music\.apple\.com\/[a-z]{2}\/artist\/[^/?#]+\/\d+/ },
  { key: 'melon', label: '멜론', placeholder: 'https://www.melon.com/artist/detail.htm?artistId=…', re: /^https:\/\/(www\.|m2?\.)?melon\.com\/.*artistId=\d+/ },
];

export function profileLinkIssue(key: 'spotify' | 'apple' | 'melon', url: string): string {
  const t = url.trim();
  if (!t) return '';
  const p = PROFILE_LINKS.find(l => l.key === key)!;
  return p.re.test(t) ? '' : `${p.label} 아티스트 페이지 주소가 아니에요. 아티스트 페이지에서 ‘링크 복사’한 주소를 붙여 넣어 주세요.`;
}

// ---------------------------------------------------------------------------
// 커버아트
// ---------------------------------------------------------------------------
export const COVER_MIN = 3000;
export const COVER_MAX = 6000;

export function coverIssue(width: number, height: number): string {
  if (width !== height) return `정사각형이 아니에요 (${width}×${height}). 1:1 비율로 다시 저장해 주세요.`;
  if (width < COVER_MIN) return `해상도가 ${width}×${height}예요. 플랫폼 기준에 맞게 ${COVER_MIN}×${COVER_MIN} 이상으로 올려 주세요.`;
  if (width > COVER_MAX) return `해상도가 너무 커요 (${width}×${height}). ${COVER_MAX}×${COVER_MAX} 이하로 줄여 주세요.`;
  return '';
}

export const mmssToSeconds = (v: string): number => {
  const m = /^(\d+):(\d{2})$/.exec(v || '');
  return m ? +m[1] * 60 + +m[2] : 0;
};
