// 오류 칸 보여 주기 — 제출했는데 빠진 칸이 있으면 화면이 그 칸으로 순간이동하지 않고 부드럽게 굴러가서
// 커서를 두고, 칸을 짧게 한 번 흔들어 "여기예요"를 알려 준다. 동작 줄이기 설정이면 바로 옮기고 흔들지 않는다.

const reduced = () => !!window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;

/** 위 고정 머리·아래 고정 버튼에 가리지 않고 다 보이는지 */
function inView(el: HTMLElement) {
  const r = el.getBoundingClientRect();
  return r.top >= 72 && r.bottom <= window.innerHeight - 96;
}

/** 굴러가기가 멈추면(스크롤이 잠시 조용해지면) 한 번 부른다. 어떤 스크롤 상자든 받는다 */
function afterScroll(cb: () => void) {
  let quiet = 0;
  let done = false;
  const finish = () => {
    if (done) return;
    done = true;
    window.removeEventListener('scroll', onScroll, true);
    window.clearTimeout(quiet);
    window.clearTimeout(limit);
    cb();
  };
  const onScroll = () => { window.clearTimeout(quiet); quiet = window.setTimeout(finish, 90); };
  window.addEventListener('scroll', onScroll, { capture: true, passive: true });
  quiet = window.setTimeout(finish, 140);
  const limit = window.setTimeout(finish, 900);
}

/** 칸을 짧게 좌우로 흔든다 — 처음엔 크게, 점점 작게 잦아든다 */
export function shake(el: Element | null) {
  if (!el || reduced() || typeof (el as HTMLElement).animate !== 'function') return;
  (el as HTMLElement).animate(
    [
      { transform: 'translateX(0)' }, { transform: 'translateX(-5px)' }, { transform: 'translateX(4px)' },
      { transform: 'translateX(-3px)' }, { transform: 'translateX(2px)' }, { transform: 'translateX(0)' },
    ],
    { duration: 380, easing: 'cubic-bezier(.36,.07,.19,.97)' },
  );
}

/** 오류 칸으로 부드럽게 옮겨 커서를 두고 한 번 흔든다 */
export function revealField(el: HTMLElement | null) {
  if (!el) return;
  const land = () => { el.focus({ preventScroll: true }); shake(el); };
  if (inView(el)) { land(); return; }
  if (reduced()) { el.scrollIntoView({ block: 'center' }); land(); return; }
  el.scrollIntoView({ behavior: 'smooth', block: 'center' });
  afterScroll(land);
}
