// 서버·다른 사이트에서 받은 주소로 이동하거나 원본 HTML에 넣기 전 확인 — 스크립트를 실행하는 주소는 막는다.
// React는 href의 javascript:를 막지만 location.assign·직접 만든 HTML은 막지 않는다.
// 브라우저는 주소 앞 공백과 중간의 탭·줄바꿈을 지우고 읽으므로(java\tscript:) 같은 방식으로 정리한 뒤 본다.
const RUNS_CODE = /^(javascript|data|vbscript|blob|file):/i;

/** 이동해도 되는 주소면 그대로, 아니면 undefined (앱 전용 주소 claude:// 등은 허용) */
export function navigableUrl(url: unknown): string | undefined {
  if (typeof url !== 'string' || !url.trim()) return undefined;
  const normalized = url.replace(/[\u0000- \u007f]/g, '');
  return RUNS_CODE.test(normalized) ? undefined : url;
}

/** http(s) 주소만 (원본 HTML의 href·src, 다른 사이트로 나가는 링크) */
export function webUrl(url: unknown): string | undefined {
  const u = navigableUrl(url);
  return u && /^https?:\/\//i.test(u.trim()) ? u : undefined;
}

/** CSS url("…") 안에 넣을 주소 — 따옴표·역슬래시·줄바꿈이 값을 끊지 못하게 퍼센트로 바꾼다 */
export function cssUrl(url: string): string {
  return `url("${url.replace(/["\\\n\r]/g, c => encodeURIComponent(c))}")`;
}
