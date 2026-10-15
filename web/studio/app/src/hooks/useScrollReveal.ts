// 스크롤 등장 모션 — 처음 화면 아래에 있던 카드·섹션만 한 번 떠오른다 (위쪽 콘텐츠는 지연 없이 바로 표시).
// 화면에 들어오기 120px 전부터 미리 시작하고(빠르게 내려도 빈 바탕이 먼저 보이지 않게) 0.28초로 짧게,
// 한꺼번에 들어오는 카드도 앞 3개까지만 0.025초 간격으로 차례로 나온다.
import { useEffect } from 'react';

const TARGETS = [
  '.surface', '.aq-report-card', '.section-top', '.aq-rp-card',
  '.aq-doc-list', '.studio-doc-path', '.aq-profile-card', '.aq-event-hero',
  '.aq-notice-featured-list', '.settle-hero', '.aq-detail-hero', '.split > *',
].join(',');
const EASE = 'cubic-bezier(.16,1,.3,1)'; // --aq-enter

export function useScrollReveal(key: string) {
  useEffect(() => {
    const reduced = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;
    if (reduced || !('IntersectionObserver' in window) || !Element.prototype.animate) return;
    const root = document.getElementById('main');
    if (!root) return;
    const seen = new WeakSet<Element>();
    const io = new IntersectionObserver(entries => {
      let order = 0;
      for (const e of entries) {
        if (!e.isIntersecting) continue;
        io.unobserve(e.target);
        const el = e.target as HTMLElement;
        el.classList.remove('aq-reveal-wait');
        el.animate(
          [{ opacity: 0, transform: 'translateY(8px)' }, { opacity: 1, transform: 'none' }],
          { duration: 280, delay: Math.min(order++, 2) * 25, easing: EASE, fill: 'backwards' },
        );
      }
    }, { rootMargin: '0px 0px 120px 0px', threshold: 0.01 });

    const scan = () => {
      const vh = window.innerHeight;
      root.querySelectorAll<HTMLElement>(TARGETS).forEach(el => {
        if (seen.has(el)) return;
        seen.add(el);
        // 이미 화면 안에 있는 요소는 건드리지 않는다
        if (el.getBoundingClientRect().top < vh * 0.92) return;
        // 부모가 이미 등장 대상이면 중복 모션을 피한다
        if (el.parentElement?.closest('.aq-reveal-wait')) return;
        el.classList.add('aq-reveal-wait');
        io.observe(el);
      });
    };
    // 비동기로 그려지는 목록도 잡기 위해 잠시 동안 DOM 변화를 지켜본다
    const t = window.setTimeout(scan, 60);
    const mo = new MutationObserver(() => scan());
    mo.observe(root, { childList: true, subtree: true });
    const stop = window.setTimeout(() => mo.disconnect(), 4000);
    return () => {
      window.clearTimeout(t); window.clearTimeout(stop);
      mo.disconnect(); io.disconnect();
      root.querySelectorAll('.aq-reveal-wait').forEach(el => el.classList.remove('aq-reveal-wait'));
    };
  }, [key]);
}
