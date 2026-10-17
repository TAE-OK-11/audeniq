import { useLayoutEffect, useRef, type RefObject } from 'react';

/**
 * 불러오기가 끝나 덩어리 높이가 바뀌면 한 번에 툭 바뀌지 않고 부드럽게 자라거나 줄어든다.
 * 자리표시(스켈레톤)와 실제 내용의 높이가 다르거나, 데이터에 따라 버튼이 생기고 없어지는 곳 — 아래 내용이 함께 따라 움직인다.
 * key가 바뀔 때만 움직인다(창 크기 변경 등은 그대로).
 */
export function useSmoothHeight<T extends HTMLElement>(ref: RefObject<T | null>, key: unknown) {
  const last = useRef(0);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const h = el.getBoundingClientRect().height;
    const from = last.current;
    last.current = h;
    if (!from || Math.abs(from - h) < 2 || window.matchMedia?.('(prefers-reduced-motion: reduce)').matches) return;
    const prev = el.style.overflow;
    el.style.overflow = 'hidden';
    // tokens.css --aq-enter · --aq-motion-long
    const anim = el.animate([{ height: `${from}px` }, { height: `${h}px` }], { duration: 320, easing: 'cubic-bezier(.32,.72,0,1)' });
    const done = () => { el.style.overflow = prev; };
    anim.onfinish = done;
    anim.oncancel = done;
  }, [ref, key]);
}
