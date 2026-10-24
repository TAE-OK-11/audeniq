// 아티스트 페이지에 직접 넣는 링크 — 주소로 로고를 고르고(있는 것만), 줄 아래에 보일 짧은 주소.
import { BRAND_ASSETS } from './brands';

export interface OwnLink { label: string; url: string }
export const OWN_LINKS_MAX = 10;
export const OWN_LABEL_MAX = 40;

/** 자주 넣는 링크 — 편집 창의 빠른 추가 칩 */
export const LINK_PRESETS = ['인스타그램', '유튜브', '틱톡', '공연 예매', '굿즈샵', '팬카페'] as const;

const HOSTS: [string, string][] = [
  ['youtube.com', 'youtube'], ['youtu.be', 'youtube'], ['tiktok.com', 'tiktok'], ['soundcloud.com', 'soundcloud'],
  ['open.spotify.com', 'spotify'], ['music.apple.com', 'apple'], ['melon.com', 'melon'], ['genie.co.kr', 'genie'],
  ['music-flo.com', 'flo'], ['bugs.co.kr', 'bugs'], ['vibe.naver.com', 'vibe'],
];

function host(url: string): string {
  try { return new URL(url).hostname.replace(/^www\./, '').toLowerCase(); } catch { return ''; }
}

/** 로고가 있는 곳이면 그 키 (BrandMark id) */
export function linkBrand(url: string): string | undefined {
  const h = host(url);
  const hit = HOSTS.find(([d]) => h === d || h.endsWith(`.${d}`));
  return hit && BRAND_ASSETS[hit[1]] ? hit[1] : undefined;
}

/** 줄 아래 작은 글자 — 'instagram.com/serin' */
export function shortUrl(url: string): string {
  try {
    const u = new URL(url);
    const path = u.pathname.replace(/\/$/, '');
    return `${u.hostname.replace(/^www\./, '')}${path}`.slice(0, 48);
  } catch { return url; }
}

/** 저장 전 검사 — 서버와 같은 기준(https, 이름 있음). 빈 줄은 뺀다. 문제가 있으면 그 줄 번호와 이유 */
export function checkLinks(links: OwnLink[]): { links: OwnLink[]; error?: { index: number; message: string } } {
  const out: OwnLink[] = [];
  for (const [i, l] of links.entries()) {
    const label = l.label.trim();
    const url = l.url.trim();
    if (!label && !url) continue;
    if (!label) return { links: out, error: { index: i, message: '링크 이름을 적어 주세요.' } };
    if (label.length > OWN_LABEL_MAX) return { links: out, error: { index: i, message: `이름은 ${OWN_LABEL_MAX}자까지 적을 수 있어요.` } };
    let ok = false;
    try { ok = new URL(url).protocol === 'https:'; } catch { ok = false; }
    if (!ok) return { links: out, error: { index: i, message: 'https://로 시작하는 주소를 넣어 주세요.' } };
    out.push({ label, url });
  }
  return { links: out };
}
