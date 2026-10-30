/** iOS 26 Safari: 키보드(또는 날짜·선택 칸의 휠)가 내려간 뒤에도 화면이 키보드 높이만큼 페이지 끝 너머로 밀린 채 남았다.
 *  발매 신청 4단계(발매일·UPC 칸)에서 아래로 내리면 '다음으로'가 화면 중간에 떠 있고 그 아래가 크게 비었다(스크린샷).
 *  키보드가 내려가면 보이는 화면 맨 위를 페이지가 실제로 스크롤될 수 있는 끝 안으로 다시 맞춘다. 밀려 있지 않으면 건드리지 않는다. */
export function settleViewport(win: Window = window) {
  const vv = win.visualViewport;
  const doc = win.document.scrollingElement ?? win.document.documentElement;
  const max = Math.max(0, doc.scrollHeight - win.innerHeight);
  const top = win.scrollY + (vv ? vv.offsetTop : 0);
  if (top <= max + 1 && win.scrollY <= max + 1) return false;
  const to = Math.min(top, max);
  // 같은 위치로의 scrollTo는 사파리가 무시해 밀린 화면이 그대로 남는다 — 한 칸 옮겼다가 제자리로
  if (Math.abs(win.scrollY - to) < 1) win.scrollTo(win.scrollX, Math.max(0, to - 1));
  win.scrollTo(win.scrollX, to);
  return true;
}

const typing = (el: Element | null) => !!el && ((el as HTMLElement).isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName));

/** 키보드가 떠 있는 동안 html에 붙는 표시 — 아래 고정 버튼 줄(.step-actions)을 그동안 숨긴다(design-align.css).
 *  iOS 사파리는 키보드가 내려간 직전·직후 잠깐 그 줄을 화면 중간(키보드가 있던 자리 위)에 그렸다가 제자리로 돌린다.
 *  키보드가 올라가면 숨기고, 내려간 뒤 화면을 다시 맞춘 다음(settleViewport) 부드럽게 다시 보인다 */
export const KB_OPEN = 'is-kb-open';

export function initKeyboardSettle() {
  if (!window.matchMedia?.('(pointer: coarse)').matches) return;
  const root = document.documentElement;
  let timers: number[] = [];
  const keyboardUp = (vv: VisualViewport) => vv.height < window.innerHeight - 80;
  // 키보드가 내려가는 움직임이 끝날 때까지 몇 번 다시 맞춘다. 두 번째로 맞춘 뒤 버튼 줄을 다시 보인다
  const run = () => {
    timers.forEach(t => window.clearTimeout(t));
    timers = [0, 150, 400].map(t => window.setTimeout(() => {
      if (typing(document.activeElement)) return;
      settleViewport();
      if (t >= 150) root.classList.remove(KB_OPEN);
    }, t));
  };
  // focusout 시점에는 다음 칸이 아직 정해지지 않았다 — 칸에서 칸으로 옮기는 중이면 그대로 둔다
  document.addEventListener('focusout', () => window.setTimeout(() => { if (!typing(document.activeElement)) run(); }, 0));
  const vv = window.visualViewport;
  vv?.addEventListener('resize', () => {
    if (keyboardUp(vv)) {
      if (typing(document.activeElement)) { timers.forEach(t => window.clearTimeout(t)); root.classList.add(KB_OPEN); }
    } else if (!typing(document.activeElement)) run();
  });
}
