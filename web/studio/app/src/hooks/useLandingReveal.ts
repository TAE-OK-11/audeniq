// 홈 등장 연출 — audeniq.com 랜딩과 같은 움직임만 가져온다 (색·배치는 스튜디오 그대로).
// 덩어리가 16px 아래에서 떠오르고, 첫 화면은 날짜 → 이름 → 인사 → 큰 카드 순으로 몇십 ms씩 차이를 둔다(0.6초).
// 화면 밖 덩어리는 스크롤로 화면에 닿기 조금 전에 시작해 더 짧게(0.48초) — 화면 안에 들어온 뒤에야 시작하면
// 내리는 동안 카드가 흐린 채로 보여 덜 불러온 것처럼 느껴졌다.
// 초점이 들어가면 바로 제자리, 탭을 숨기면 진행 중인 연출을 끝낸다. 동작 줄이기·미지원 브라우저에서는 아무것도 하지 않는다.
// (docs/DESIGN_SYSTEM.md 규칙 19의 예외 — 홈 첫 화면에만 쓴다. 다른 화면의 짧은 스크롤 등장은 useScrollReveal)
import { useLayoutEffect, useRef, type RefObject } from 'react';
import { entryMotion, hasEntranceAnimation, rise, type EntryMotion } from './useScrollReveal';

export interface RevealGroup {
  selector: string;
  /** i: 그룹 안 순서, first: 처음부터 화면 안에 있던 덩어리인지(첫 화면은 머리 다음에 차례로) */
  delay?: (i: number, el: HTMLElement, first: boolean) => number;
}

const EASING = 'cubic-bezier(.22,.61,.36,1)';
const SCROLL_MS = 480;
const SCROLL_RISE = rise(16);
const WAITING = 'aq-motion-waiting';

/** key가 바뀔 때마다(불러오기가 끝나 내용이 그려지면) 아직 나타나지 않은 덩어리만 새로 맡는다 — 한 번 나타난 것은 다시 움직이지 않는다 */
export function useLandingReveal(root: RefObject<HTMLElement | null>, groups: RevealGroup[], key: unknown) {
  const seen = useRef(new WeakSet<HTMLElement>());
  const firstRun = useRef(true);
  // 앱을 연 첫 화면이면 길게, 다른 화면에서 옮겨 왔으면 짧게 — 불러오기 뒤 다시 돌 때도 처음 정한 크기를 쓴다
  const entry = useRef<EntryMotion | null>(null);

  // 그리기 전에 숨긴다 — 한 프레임이라도 제자리에 보였다가 사라지지 않게
  useLayoutEffect(() => {
    const box = root.current;
    const reduced = matchMedia('(prefers-reduced-motion: reduce)').matches;
    if (!box || reduced || typeof IntersectionObserver === 'undefined' || !box.animate) return;
    const initial = firstRun.current;
    firstRun.current = false;
    const motion = (entry.current ??= entryMotion());

    const delays = new Map<HTMLElement, number>();
    const firsts = new Set<HTMLElement>();
    const onScreen = new Set<HTMLElement>();
    const cssEntrance = new Set<HTMLElement>();
    for (const g of groups) {
      box.querySelectorAll<HTMLElement>(g.selector).forEach((el, i) => {
        if (seen.current.has(el) || delays.has(el)) return;
        // 처음 그릴 때 이미 보이는 덩어리만 첫 화면 순서를 따른다 (불러온 뒤 나타난 카드도 화면 안이면 같은 흐름)
        const r = el.getBoundingClientRect();
        const inView = r.top < window.innerHeight;
        if (inView && r.bottom > 0) onScreen.add(el);
        if (hasEntranceAnimation(el)) cssEntrance.add(el);
        const first = inView && (initial || window.scrollY < 40);
        if (first) firsts.add(el);
        delays.set(el, Math.round((g.delay?.(i, el, first) ?? 0) * (first ? motion.pace : 1)));
      });
    }

    const pending = new Set<HTMLElement>();
    const active = new Map<HTMLElement, Animation>();
    const show = (el: HTMLElement, now = false) => {
      if (!pending.delete(el)) return;
      io.unobserve(el);
      seen.current.add(el);
      // 연출을 먼저 걸고 숨김을 푼다 — 한 프레임이라도 제자리에 보였다가 다시 내려가지 않게
      if (!now && !document.hidden) {
        const first = firsts.has(el);
        const a = el.animate(first ? motion.keyframes : SCROLL_RISE, { duration: first ? motion.ms : SCROLL_MS, easing: EASING, fill: 'backwards', delay: delays.get(el) ?? 0 });
        active.set(el, a);
        a.finished.then(() => { if (active.get(el) === a) active.delete(el); }, () => {});
      }
      el.classList.remove(WAITING);
    };
    const io = new IntersectionObserver(entries => {
      entries.forEach(e => { if (e.isIntersecting) show(e.target as HTMLElement); });
    }, { threshold: 0, rootMargin: '0px 0px 64px 0px' });

    delays.forEach((_, el) => {
      if (el.contains(document.activeElement)) { seen.current.add(el); return; }
      // 자체 CSS 등장 애니메이션과 겹치지 않게 끄고, 이미 화면 안이면 보임 신호를 기다리지 않는다 (useScrollReveal과 같은 이유)
      if (cssEntrance.has(el)) el.style.animation = 'none';
      pending.add(el);
      el.classList.add(WAITING);
      if (onScreen.has(el)) show(el);
      else io.observe(el);
    });

    const settle = () => { active.forEach(a => a.cancel()); active.clear(); };
    const showAll = () => { [...pending].forEach(el => show(el, true)); settle(); };
    const onFocus = (e: FocusEvent) => {
      const t = e.target as Node;
      [...pending].forEach(el => { if (el.contains(t)) show(el, true); });
      active.forEach((a, el) => { if (el.contains(t)) { a.cancel(); active.delete(el); } });
    };
    const onVisibility = () => { if (document.hidden) settle(); };
    document.addEventListener('focusin', onFocus);
    document.addEventListener('visibilitychange', onVisibility);
    window.addEventListener('beforeprint', showAll);

    return () => {
      io.disconnect();
      document.removeEventListener('focusin', onFocus);
      document.removeEventListener('visibilitychange', onVisibility);
      window.removeEventListener('beforeprint', showAll);
      // 아직 나타나지 않은 것은 다음 차례(새로 그린 내용)에서 다시 맡는다 — 같은 커밋 안이라 깜빡이지 않는다
      pending.forEach(el => el.classList.remove(WAITING));
    };
  }, [key]); // eslint-disable-line react-hooks/exhaustive-deps
}
