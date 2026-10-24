// 홈 등장 연출 — audeniq.com 랜딩과 같은 움직임만 가져온다 (색·배치는 스튜디오 그대로).
// 덩어리가 화면에 들어올 때 16px 아래에서 0.76초 동안 떠오르고, 첫 화면은 날짜 → 이름 → 인사 → 큰 카드 순으로
// 몇십 ms씩 차이를 둔다. 화면 밖 덩어리는 미리 숨겨 두었다가 스크롤해서 보일 때 나타난다.
// 초점이 들어가면 바로 제자리, 탭을 숨기면 진행 중인 연출을 끝낸다. 동작 줄이기·미지원 브라우저에서는 아무것도 하지 않는다.
// (docs/DESIGN_SYSTEM.md 규칙 19의 예외 — 홈 첫 화면에만 쓴다. 다른 화면의 짧은 스크롤 등장은 useScrollReveal)
import { useEffect, useRef, type RefObject } from 'react';

export interface RevealGroup {
  selector: string;
  /** i: 그룹 안 순서, first: 처음부터 화면 안에 있던 덩어리인지(첫 화면은 머리 다음에 차례로) */
  delay?: (i: number, el: HTMLElement, first: boolean) => number;
}

const KEYFRAMES: Keyframe[] = [{ opacity: 0, transform: 'translateY(16px)' }, { opacity: 1, transform: 'none' }];
const TIMING = { duration: 760, easing: 'cubic-bezier(.22,.61,.36,1)', fill: 'backwards' as const };
const WAITING = 'aq-motion-waiting';

/** key가 바뀔 때마다(불러오기가 끝나 내용이 그려지면) 아직 나타나지 않은 덩어리만 새로 맡는다 — 한 번 나타난 것은 다시 움직이지 않는다 */
export function useLandingReveal(root: RefObject<HTMLElement | null>, groups: RevealGroup[], key: unknown) {
  const seen = useRef(new WeakSet<HTMLElement>());
  const firstRun = useRef(true);

  useEffect(() => {
    const box = root.current;
    const reduced = matchMedia('(prefers-reduced-motion: reduce)').matches;
    if (!box || reduced || typeof IntersectionObserver === 'undefined' || !box.animate) return;
    const initial = firstRun.current;
    firstRun.current = false;

    const delays = new Map<HTMLElement, number>();
    for (const g of groups) {
      box.querySelectorAll<HTMLElement>(g.selector).forEach((el, i) => {
        if (seen.current.has(el) || delays.has(el)) return;
        // 처음 그릴 때 이미 보이는 덩어리만 첫 화면 순서를 따른다 (불러온 뒤 나타난 카드도 화면 안이면 같은 흐름)
        const inView = el.getBoundingClientRect().top < window.innerHeight;
        delays.set(el, g.delay?.(i, el, inView && (initial || window.scrollY < 40)) ?? 0);
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
        const a = el.animate(KEYFRAMES, { ...TIMING, delay: delays.get(el) ?? 0 });
        active.set(el, a);
        a.finished.then(() => { if (active.get(el) === a) active.delete(el); }, () => {});
      }
      el.classList.remove(WAITING);
    };
    const io = new IntersectionObserver(entries => {
      entries.forEach(e => { if (e.isIntersecting) show(e.target as HTMLElement); });
    }, { threshold: 0, rootMargin: '0px 0px -10% 0px' });

    delays.forEach((_, el) => {
      if (el.contains(document.activeElement)) { seen.current.add(el); return; }
      pending.add(el);
      el.classList.add(WAITING);
      io.observe(el);
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
