/* AUDENIQ Survey motion — matched to the public homepage's reveal timing (ported from public/motion.js).
 * Runs only in the visitor's browser. No network or Worker API calls.
 */
const reduced = window.matchMedia('(prefers-reduced-motion: reduce)');
const small = window.matchMedia('(max-width: 760px)');
const active = new Map<Element, Animation>();
const pending = new Set<Element>();
const timing = new Map<Element, number>();
const ease = 'cubic-bezier(.22,.61,.36,1)';
let observer: IntersectionObserver | undefined;

function settle(el: Element) {
  const animation = active.get(el);
  if (animation) {
    animation.cancel();
    active.delete(el);
  }
}
function enter(el: Element | null, delay = 0, distance = 16, duration = 760) {
  if (!el || !el.isConnected || el.closest('[hidden]') || reduced.matches || document.hidden || !el.animate) return;
  settle(el);
  const y = Math.min(distance, small.matches ? 12 : 16);
  const animation = el.animate([
    { opacity: 0, transform: `translateY(${y}px)` },
    { opacity: 1, transform: 'none' }
  ], { duration, delay, easing: ease, fill: 'backwards' });
  active.set(el, animation);
  animation.finished.then(() => { if (active.get(el) === animation) active.delete(el); }, () => {});
}
function reveal(el: Element, immediate = false) {
  if (!pending.has(el)) return;
  pending.delete(el);
  observer?.unobserve(el);
  if (!immediate && !el.contains(document.activeElement)) enter(el, timing.get(el) || 0);
  el.classList.remove('motion-waiting');
  timing.delete(el);
}
function revealAll() {
  observer?.disconnect();
  for (const el of [...pending]) reveal(el, true);
  for (const el of active.keys()) settle(el);
}
function watch(el: Element | null, delay = 0) {
  if (!el || el.closest('[hidden]') || reduced.matches || document.hidden || !observer || !el.animate) return;
  if (el.contains(document.activeElement)) return;
  pending.add(el);
  timing.set(el, delay);
  el.classList.add('motion-waiting');
  observer.observe(el);
}
function scheduleStep(step: HTMLElement | null) {
  if (!step || step.hidden || reduced.matches) return;
  // Only the visible step is animated. Inputs and checkbox labels never move.
  const heading = [...step.children].filter(el => el.matches('.step-kicker, h2, .step-desc'));
  heading.forEach((el, i) => watch(el, i * 65));
  step.querySelectorAll(':scope > .question:not([hidden])').forEach((el, i) => watch(el, small.matches ? 0 : i % 2 * 70));
}
export function initializeMotion() {
  if (reduced.matches || !('IntersectionObserver' in window) || !Element.prototype.animate) return;
  try {
    observer = new IntersectionObserver(entries => {
      for (const entry of entries) if (entry.isIntersecting) reveal(entry.target);
    }, { threshold: 0, rootMargin: '0px 0px -8% 0px' });
    const intro = document.querySelector('.intro');
    if (intro) [...intro.children].forEach((el, i) => watch(el, Math.min(i, 3) * 65));
    const progress = document.querySelector('.progress-head');
    const track = document.querySelector('.progress-track');
    watch(progress); watch(track, 60);
    scheduleStep(document.querySelector<HTMLElement>('.step:not([hidden])'));
  } catch { revealAll(); }
  reduced.addEventListener?.('change', () => { if (reduced.matches) revealAll(); });
  document.addEventListener('focusin', ({ target }) => {
    const node = target as Node | null;
    for (const el of [...pending]) if (el.contains(node)) reveal(el, true);
    for (const el of [...active.keys()]) if (el.contains(node)) settle(el);
  });
  document.addEventListener('visibilitychange', () => {
    if (document.hidden) revealAll();
  });
  window.addEventListener('pagehide', revealAll);
  window.addEventListener('beforeprint', revealAll);
  window.addEventListener('pageshow', ({ persisted }) => { if (persisted) revealAll(); });
}
export function animateSurveyStep(step: HTMLElement | null) {
  // Clear old animations before hiding / changing form step to avoid stale layers.
  for (const el of [...pending]) if (el.closest('.step')) reveal(el, true);
  for (const el of [...active.keys()]) if (el.closest('.step')) settle(el);
  scheduleStep(step);
}
export function animateSurveyComplete(complete: HTMLElement | null) {
  revealAll();
  if (!reduced.matches && complete && !document.hidden) {
    enter(complete, 0, 16, 700);
    enter(complete.querySelector('.complete-icon'), 90, 12, 550);
  }
}
