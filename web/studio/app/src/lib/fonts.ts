// 글꼴 두 번째 조각(more) — 화면 문구에 없는 글자(사용자가 입력한 이름·제목 등)가 들어 있다 (build/fonts.py).
// index.html은 첫 화면에 쓰는 core만 미리 받는다. 이름이 나올 화면(로그인 뒤 스튜디오·공개 페이지)에서 이 함수로 미리 받아 두면
// 그 글자가 잠깐 다른 글꼴로 보였다 바뀌는 일이 줄어든다. 로그인·가입 화면은 받지 않는다(core만 237 KB).
let started = false;
export function preloadMoreFont(): void {
  if (started || typeof document === 'undefined') return;
  // @font-face로 선언된 more 조각을 바로 받게 한다 — <link rel=preload>는 우선순위가 높아 화면 조각과 다투고,
  // 몇 초 안에 그 글자가 안 나오면 브라우저가 '받고 쓰지 않음' 경고를 남겼다. FontFace.load()는 보통 순위로 받고 사파리도 지원한다
  const face = [...(document.fonts ?? [])].find(f => f.family.replace(/["']/g, '') === 'Audeniq Sans' && /U\+0-10FFFF/i.test(f.unicodeRange));
  if (face) {
    started = true;
    void face.load().catch(() => { /* 오프라인 등 — 글자가 나올 때 다시 받는다 */ });
    return;
  }
  const css = document.getElementById('aq-fonts')?.textContent ?? '';
  const url = /url\("([^"]*audeniq-sans-more[^"]*)"\)/.exec(css)?.[1];
  if (!url) return;
  started = true;
  const link = document.createElement('link');
  link.rel = 'preload';
  link.as = 'font';
  link.type = 'font/woff2';
  link.crossOrigin = 'anonymous';
  link.href = url;
  document.head.appendChild(link);
}
