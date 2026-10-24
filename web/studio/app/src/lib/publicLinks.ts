// 공개 페이지 주소 — 발매 스마트링크 /link/release/{코드}, 아티스트 소개 /link/artist/{주소 이름}(+ /press·/news·/discography).
// 예전 주소(/l/{코드}, /a/{코드})는 App.tsx에서 새 주소로 넘긴다 (이미 나간 QR·링크가 계속 열리게).
export const releasePath = (slug: string) => `/link/release/${encodeURIComponent(slug)}`;
export const artistPath = (handle: string) => `/link/artist/${encodeURIComponent(handle)}`;
export const artistPressPath = (handle: string) => `${artistPath(handle)}/press`;
export const artistNewsPath = (handle: string) => `${artistPath(handle)}/news`;
export const artistDiscographyPath = (handle: string) => `${artistPath(handle)}/discography`;

export const releaseUrl = (slug: string) => `${window.location.origin}${releasePath(slug)}`;
export const artistUrl = (handle: string) => `${window.location.origin}${artistPath(handle)}`;
export const artistPressUrl = (handle: string) => `${window.location.origin}${artistPressPath(handle)}`;

/** 아티스트가 고르는 주소 이름 — 서버(artist_links::valid_handle)와 같은 규칙. 앞의 @는 떼고 소문자로 */
export const HANDLE_MIN = 3;
export const HANDLE_MAX = 30;
export function normalizeHandle(raw: string): string {
  return raw.trim().replace(/^@/, '').toLowerCase();
}
export function handleProblem(h: string): string | null {
  if (h.length < HANDLE_MIN || h.length > HANDLE_MAX) return `${HANDLE_MIN}–${HANDLE_MAX}자로 적어 주세요.`;
  if (!/^[a-z0-9_-]+$/.test(h)) return '영문 소문자·숫자·_·-만 쓸 수 있어요.';
  if (!/^[a-z0-9]/.test(h) || !/[a-z0-9]$/.test(h)) return '처음과 끝은 영문이나 숫자여야 해요.';
  return null;
}
