// 홈 스크롤 등장 모션 — 홍보 페이지(audeniq.com, web/landing/index.html)와 같은 움직임.
// data-reveal이 붙은 요소가 화면에 들어올 때 16px(휴대폰 12px) 아래에서 떠오르고, data-delay(ms)만큼 차례로 나온다.
// 처음 화면에 이미 보이는 요소는 화면 전환 모션(view-enter)이 맡으니 건드리지 않는다.
// 스크립트·IntersectionObserver가 없거나 동작 줄이기 설정이면 그냥 보인다
// (모션이 보이는지를 정하지 않는다 — 애니메이션은 끝나면 버린다).
import { useEffect, type RefObject } from 'react';

const EASE = 'cubic-bezier(.22,.61,.36,1)';
const DURATION = 760;

export function useLandingReveal(root: RefObject<HTMLElement | null>, ready: boolean) {
  useEffect(() => {
    const host = root.current;
    if (!ready || !host) return;
    const reduced = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;
    if (reduced || !('IntersectionObserver' in window) || !Element.prototype.animate) return;
    const distance = window.matchMedia('(max-width: 760px)').matches ? 12 : 16;
    const seen = new WeakSet<Element>();
    const play = (el: HTMLElement) => {
      el.classList.remove('aq-rise-wait');
      el.animate(
        [{ opacity: 0, transform: `translateY(${distance}px)` }, { opacity: 1, transform: 'none' }],
        { duration: DURATION, delay: Number(el.dataset.delay) || 0, easing: EASE, fill: 'backwards' },
      );
    };
    const io = new IntersectionObserver(entries => {
      for (const e of entries) {
        if (!e.isIntersecting) continue;
        io.unobserve(e.target);
        play(e.target as HTMLElement);
      }
    }, { threshold: 0, rootMargin: '0px 0px -10% 0px' });
    const scan = () => {
      host.querySelectorAll<HTMLElement>('[data-reveal]').forEach(el => {
        if (seen.has(el)) return;
        seen.add(el);
        // 키보드로 들어온 칸, 이미 화면 안에 있는 칸은 숨기지 않는다
        if (el.contains(document.activeElement) || el.getBoundingClientRect().top < window.innerHeight * 0.92) return;
        el.classList.add('aq-rise-wait');
        io.observe(el);
      });
    };
    scan();
    // 늦게 그려지는 목록(할 일·발매)도 잡는다
    const mo = new MutationObserver(scan);
    mo.observe(host, { childList: true, subtree: true });
    const stop = window.setTimeout(() => mo.disconnect(), 4000);
    const onFocus = (e: FocusEvent) => {
      host.querySelectorAll<HTMLElement>('.aq-rise-wait').forEach(el => {
        if (el.contains(e.target as Node)) { io.unobserve(el); el.classList.remove('aq-rise-wait'); }
      });
    };
    host.addEventListener('focusin', onFocus);
    return () => {
      window.clearTimeout(stop); mo.disconnect(); io.disconnect();
      host.removeEventListener('focusin', onFocus);
      host.querySelectorAll('.aq-rise-wait').forEach(el => el.classList.remove('aq-rise-wait'));
    };
  }, [root, ready]);
}
