/** iOS 26 Safari: 키보드가 뜨면 고정·sticky 레이어(발매 신청 머리, .aq-tint-top 흰 띠)는 줄어든 화면 안에만 그리고
 *  위 상태 표시줄 칸에는 페이지 자체만 그린다. 그래서 칸을 눌러 입력하는 동안 머리 위로 스크롤된 내용이 비쳐 보였다.
 *  고정 레이어로는 그 칸을 덮을 수 없으므로 페이지 안(absolute)에 흰 덮개를 두고, 화면 맨 위 바로 위쪽을 늘 덮도록 스크롤을 따라간다.
 *  평소에는 화면 밖에 있어 보이지 않는다. 창(Modal)이 열리면 창이 자기 덮개(coverOutsideFixedLayer)를 그 위에 깐다. */
export function initTopCover() {
  if (!window.matchMedia?.('(pointer: coarse)').matches) return;
  const cover = document.createElement('div');
  cover.setAttribute('aria-hidden', 'true');
  cover.className = 'aq-top-cover';
  cover.style.cssText = 'position:absolute;left:0;right:0;z-index:69;pointer-events:none;background:#fff';
  document.body.appendChild(cover);
  let raf = 0;
  const place = () => {
    raf = 0;
    const h = Math.max(window.innerHeight, window.screen?.height ?? 0);
    // 아래 끝은 window.scrollY(화면 맨 위)에 — visualViewport.offsetTop을 더하면 아래 끝에서 당겨 튕길 때 iOS가 그 값을
    // 내려 보내 덮개가 화면 안으로 내려와 머리와 내용을 하얗게 가렸다(발매 신청 화면 녹화)
    cover.style.top = `${Math.round(window.scrollY - h)}px`;
    cover.style.height = `${h}px`;
  };
  // 덮개는 키보드가 떠 있을 때(입력 칸에 포커스)만 둔다. 스크롤을 한 박자 늦게 따라가므로 평소에 두면
  // 위로 빠르게 스크롤할 때 화면 맨 위로 내려와 내용을 하얗게 가렸다(알림 화면 녹화)
  const typing = () => {
    const el = document.activeElement as HTMLElement | null;
    return !!el && (el.isContentEditable || /^(TEXTAREA|SELECT)$/.test(el.tagName)
      || (el.tagName === 'INPUT' && !/^(checkbox|radio|button|submit|reset|range|color|file|image)$/.test((el as HTMLInputElement).type)));
  };
  const toggle = () => { cover.hidden = !typing(); if (!cover.hidden) place(); };
  toggle();
  document.addEventListener('focusin', toggle);
  // focusout 시점에는 다음 포커스가 아직 정해지지 않았다 — 칸에서 칸으로 옮길 때 깜빡이지 않게 한 박자 뒤에 본다
  document.addEventListener('focusout', () => setTimeout(toggle, 0));
  const schedule = () => { if (!raf && !cover.hidden) raf = requestAnimationFrame(place); };
  const vv = window.visualViewport;
  window.addEventListener('scroll', schedule, { passive: true });
  window.addEventListener('resize', schedule);
  vv?.addEventListener('resize', schedule);
  vv?.addEventListener('scroll', schedule);
}
