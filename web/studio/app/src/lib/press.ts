// 누름 피드백 — 누르는 순간 바로 눌린 모습(.is-pressed)이 되고, 아주 짧게 톡 눌러도 최소 시간만큼은 보였다가
// 부드럽게 돌아온다. :active만으로는 빠른 탭에서 눌린 모습이 몇 ms라 보이지 않는다.
// 손가락이 움직이면(스크롤) 바로 풀어 스크롤 중에 줄이 눌린 채로 남지 않는다. 모양은 CSS(ui.css '누름 피드백')가 정한다.

const PRESSABLE = [
  'button', 'a[href]', 'summary', '[role=button]', '[role=tab]', '[role=link]', '[role=radio]', '[role=option]',
  'label.ui-check', 'label.ui-choice', 'label.aq-choice', 'label.aq-option', 'label.aq-pick', 'label.aq-pay-type', 'label.aq-file-btn',
  '.ui-row', '.service-item',
].join(',');
/** 눌린 모습을 보여 주는 최소 시간 — 빠른 탭에서도 눌렸다는 걸 느낄 수 있게 */
const MIN_MS = 70;
/** 이만큼 움직이면 누름이 아니라 스크롤로 본다 */
const MOVE_PX = 10;

export function initPressFeedback(root: Document = document) {
  let el: HTMLElement | null = null;
  let start = 0;
  let sx = 0;
  let sy = 0;

  const drop = (target: HTMLElement, wait: number) => {
    if (wait <= 0) target.classList.remove('is-pressed');
    else window.setTimeout(() => { if (el !== target) target.classList.remove('is-pressed'); }, wait);
  };
  const release = () => {
    if (!el) return;
    const t = el;
    el = null;
    drop(t, MIN_MS - (performance.now() - start));
  };
  const cancel = () => {
    if (!el) return;
    el.classList.remove('is-pressed');
    el = null;
  };

  root.addEventListener('pointerdown', e => {
    if (e.button !== 0 || !(e.target instanceof Element)) return;
    const t = e.target.closest<HTMLElement>(PRESSABLE);
    if (!t || t.matches(':disabled,[aria-disabled=true]') || t.closest('[inert]')) return;
    if (el && el !== t) el.classList.remove('is-pressed');
    el = t;
    start = performance.now();
    sx = e.clientX;
    sy = e.clientY;
    t.classList.add('is-pressed');
  }, { passive: true, capture: true });
  root.addEventListener('pointermove', e => {
    if (el && Math.hypot(e.clientX - sx, e.clientY - sy) > MOVE_PX) cancel();
  }, { passive: true, capture: true });
  root.addEventListener('pointerup', release, { passive: true, capture: true });
  root.addEventListener('pointercancel', cancel, { passive: true, capture: true });
  root.addEventListener('scroll', cancel, { passive: true, capture: true });
  window.addEventListener('blur', cancel);
}
