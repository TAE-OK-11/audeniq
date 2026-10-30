// web/studio/app/src/lib/fonts.ts와 같은 코드
// 글꼴 두 번째 조각(more) — 화면 문구에 없는 글자(사용자가 입력한 이름·제목 등)가 들어 있다 (build/fonts.py).
// index.html은 첫 화면에 쓰는 core만 미리 받는다. 이름이 나올 화면(로그인 뒤 콘솔 — 심사 목록에 아티스트·곡 이름이 바로 나온다)에서 이 함수로 미리 받아 두면
// 그 글자가 잠깐 다른 글꼴로 보였다 바뀌는 일이 줄어든다. 로그인 화면은 받지 않는다(core만 184 KB).
let started = false;
export function preloadMoreFont(): void {
  if (started || typeof document === 'undefined') return;
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
