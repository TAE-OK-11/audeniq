// 공개 페이지(아티스트 소개·소식·디스코그래피) 스크롤 등장 — 스튜디오의 useScrollReveal과 같은 움직임(16px 아래에서 0.48초).
// 처음 화면 안에 있던 것은 카드의 CSS 차례 등장에 맡기고 건드리지 않는다(새로고침 직후 다시 숨었다 나타나 튀지 않게).
// 화면 아래 덩어리만 그리기 전에 숨겨 두고, 스크롤로 닿기 조금 전에 띄운다. 늦게 그려지는 목록(응원·소식)도 잠시 지켜본다.
// 화면을 누르거나 키를 치면 새로 생기는 것은 더 지켜보지 않는다(거르기·더 보기로 바뀐 내용은 바로 보인다). 동작 줄이기면 아무것도 안 한다.
import { useLayoutEffect, type RefObject } from 'react';

const KEYFRAMES: Keyframe[] = [{ opacity: 0, transform: 'translateY(16px)' }, { opacity: 1, transform: 'none' }];
const WAITING = 'aq-pub-waiting';
const WATCH_MS = 4000;

export function usePublicReveal(root: RefObject<HTMLElement | null>, selector: string, ready: boolean) {
  useLayoutEffect(() => {
    const host = root.current;
    if (!ready || !host) return;
    if (window.matchMedia?.('(prefers-reduced-motion: reduce)').matches || typeof IntersectionObserver === 'undefined' || !Element.prototype.animate) return;
    const seen = new WeakSet<Element>();
    const pending = new Set<HTMLElement>();

    const show = (el: HTMLElement, animate = true) => {
      if (!pending.delete(el)) return;
      io.unobserve(el);
      el.classList.remove(WAITING);
      if (!animate || document.hidden) return;
      // 스크롤로 닿은 덩어리는 차례 없이 한 번에 (디자인 시스템 19 — 0.48초, 16px)
      el.animate(KEYFRAMES, { duration: 480, easing: 'cubic-bezier(.22,.61,.36,1)', fill: 'backwards' });
    };
    const io = new IntersectionObserver(entries => {
      entries.forEach(e => { if (e.isIntersecting) show(e.target as HTMLElement); });
    }, { rootMargin: '0px 0px 48px 0px' });

    const scan = () => {
      const fresh = Array.from(host.querySelectorAll<HTMLElement>(selector)).filter(el => !seen.has(el));
      fresh.forEach(el => seen.add(el));
      const vh = window.innerHeight;
      const tops = fresh.map(el => el.getBoundingClientRect().top);
      fresh.forEach((el, i) => {
        if (tops[i] < vh) return; // 이미 화면 안(또는 위) — 그대로
        pending.add(el);
        // 자체 CSS 등장(카드 차례)이 있으면 끈다 — 화면 밖에서 먼저 끝나 버려 의미가 없고, 겹치면 두 번 떠오른다
        el.style.animation = 'none';
        el.classList.add(WAITING);
        io.observe(el);
      });
    };
    scan();
    const mo = new MutationObserver(scan);
    mo.observe(host, { childList: true, subtree: true });
    const stopWatching = () => mo.disconnect();
    const stop = window.setTimeout(stopWatching, WATCH_MS);
    host.addEventListener('pointerdown', stopWatching, { capture: true, once: true });
    host.addEventListener('keydown', stopWatching, { capture: true, once: true });
    const onFocus = (e: FocusEvent) => { [...pending].forEach(el => { if (el.contains(e.target as Node)) show(el, false); }); };
    const showAll = () => [...pending].forEach(el => show(el, false));
    document.addEventListener('focusin', onFocus);
    window.addEventListener('beforeprint', showAll);
    return () => {
      window.clearTimeout(stop);
      mo.disconnect();
      io.disconnect();
      host.removeEventListener('pointerdown', stopWatching, { capture: true });
      host.removeEventListener('keydown', stopWatching, { capture: true });
      document.removeEventListener('focusin', onFocus);
      window.removeEventListener('beforeprint', showAll);
      pending.forEach(el => el.classList.remove(WAITING));
    };
  }, [root, selector, ready]);
}
