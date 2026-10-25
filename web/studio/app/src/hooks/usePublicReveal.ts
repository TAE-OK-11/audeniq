// 공개 화면(발매·아티스트 소개·소식·디스코그래피·보도자료) 등장 — 스튜디오 useScrollReveal과 같은 움직임.
// - 첫 화면: 카드의 덩어리(머리·커버·제목·설명·섹션)가 위에서부터 차례로 16px 아래에서 떠오른다 (0.6초,
//   앞의 넷은 50ms씩, 그 뒤는 150ms부터 60ms씩 네 개마다 다시). 바깥 덩어리를 맡으면 그 안쪽은 따로 움직이지 않는다.
// - 화면 아래 덩어리(selector — 소식 카드·발매 칸·응원 줄 등)는 그리기 전에 숨겨 두고 스크롤로 닿기 조금 전에 한 번에 떠오른다 (0.48초).
// 숨김은 그리기 전에(useLayoutEffect) — 한 프레임이라도 보였다가 사라지지 않게. 첫 화면 것은 숨기지 않고 WAAPI의
// 시작 전 채움(fill: backwards)만 쓰므로 새로고침 직후 다시 숨었다 나타나 튀지 않는다.
// 늦게 그려지는 목록(응원·소식)도 잠시 지켜보고, 화면을 누르거나 키를 치면 그만 지켜본다(거르기·더 보기로 바뀐 내용은 바로 보인다).
// 초점이 들어가면 바로 제자리, 탭을 숨기면 진행 중인 연출을 끝낸다. 동작 줄이기면 아무것도 하지 않는다.
import { useLayoutEffect, type RefObject } from 'react';

const KEYFRAMES: Keyframe[] = [{ opacity: 0, transform: 'translateY(16px)' }, { opacity: 1, transform: 'none' }];
const EASING = 'cubic-bezier(.22,.61,.36,1)';
const FIRST_MS = 600;
const SCROLL_MS = 480;
const WAITING = 'aq-pub-waiting';
const WATCH_MS = 4000;
/** 화면을 연 뒤 이 시간 안에 화면 안에 그려진 덩어리(늦게 온 목록)도 첫 화면 차례를 탄다 */
const FIRST_WINDOW = 1500;

/** 첫 화면 n번째 덩어리의 시작 지연 — 머리 넷은 50ms씩, 그 뒤 덩어리는 150ms부터 60ms씩 (네 개마다 다시) */
export function firstDelay(n: number): number {
  return n < 4 ? n * 50 : 150 + ((n - 4) % 4) * 60;
}

/** first: false — 첫 화면 차례 등장은 그곳 자체 연출(전체 메뉴 열림 등)에 맡기고, 화면 밖 덩어리의 스크롤 등장만 한다 */
export function usePublicReveal(root: RefObject<HTMLElement | null>, selector: string, ready: boolean, { first: playFirst = true }: { first?: boolean } = {}) {
  useLayoutEffect(() => {
    const host = root.current;
    if (!ready || !host) return;
    if (window.matchMedia?.('(prefers-reduced-motion: reduce)').matches || typeof IntersectionObserver === 'undefined' || !Element.prototype.animate) return;
    const seen = new WeakSet<Element>();
    const pending = new Set<HTMLElement>();
    const running = new Set<Animation>();

    const play = (el: HTMLElement, duration: number, delay = 0) => {
      if (document.hidden) return;
      const a = el.animate(KEYFRAMES, { duration, easing: EASING, fill: 'backwards', delay });
      running.add(a);
      a.finished.then(() => running.delete(a), () => running.delete(a));
    };
    const show = (el: HTMLElement, animate = true) => {
      if (!pending.delete(el)) return;
      io.unobserve(el);
      el.classList.remove(WAITING);
      if (animate) play(el, SCROLL_MS);
    };
    const io = new IntersectionObserver(entries => {
      entries.forEach(e => { if (e.isIntersecting) show(e.target as HTMLElement); });
    }, { rootMargin: '0px 0px 48px 0px' });
    const touched = new Set<HTMLElement>();
    const wait = (el: HTMLElement) => {
      pending.add(el);
      // 자체 CSS 등장(전체 메뉴 묶음 등)은 화면 밖에서 먼저 끝나 의미가 없고, 겹치면 두 번 떠오른다
      el.style.animation = 'none';
      touched.add(el);
      el.classList.add(WAITING);
      io.observe(el);
    };

    // 첫 화면 — 카드 바로 아래 덩어리를 위에서부터 차례로. 화면 밖 덩어리는 안쪽 단위(selector)가 있으면 그것들을,
    // 없으면 덩어리 통째로 스크롤 등장에 맡긴다
    const started = performance.now();
    const vh = window.innerHeight;
    const top = Array.from(host.children) as HTMLElement[];
    const rects = top.map(el => el.getBoundingClientRect());
    let n = 0;
    top.forEach((el, i) => {
      const r = rects[i];
      if (!r.width && !r.height) return;
      if (r.top < vh) {
        seen.add(el);
        el.querySelectorAll(selector).forEach(x => seen.add(x));
        if (playFirst) play(el, FIRST_MS, firstDelay(n++));
      } else if (!el.matches(selector) && !el.querySelector(selector)) {
        seen.add(el);
        wait(el);
      }
    });

    const scan = () => {
      const first = playFirst && performance.now() - started < FIRST_WINDOW && window.scrollY < 40;
      // 늦게 그려진 카드 바로 아래 덩어리(불러오기 뒤의 거르기 줄·목록) — 첫 화면 시간 안이면 같은 차례로
      (Array.from(host.children) as HTMLElement[]).filter(el => !seen.has(el)).forEach(el => {
        const r = el.getBoundingClientRect();
        if (!r.width && !r.height) return;
        // 화면 밖이면서 단위(selector)인 덩어리는 아래 단위 차례에 맡긴다 — 여기서 본 것으로 치면 그대로 보인 채 남는다
        if (r.top >= window.innerHeight && (el.matches(selector) || el.querySelector(selector))) return;
        seen.add(el);
        if (r.top < window.innerHeight) {
          if (!first) return;
          el.querySelectorAll(selector).forEach(x => seen.add(x));
          play(el, FIRST_MS, firstDelay(n++));
        } else wait(el);
      });
      const fresh = Array.from(host.querySelectorAll<HTMLElement>(selector)).filter(el => !seen.has(el));
      fresh.forEach(el => seen.add(el));
      const h = window.innerHeight;
      const tops = fresh.map(el => el.getBoundingClientRect().top);
      fresh.forEach((el, i) => {
        if (tops[i] >= h) wait(el);
        else if (first && tops[i] > 0) play(el, FIRST_MS, firstDelay(n++));
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
    const settle = () => { running.forEach(a => a.finish()); running.clear(); };
    const onVisibility = () => { if (document.hidden) settle(); };
    const showAll = () => { [...pending].forEach(el => show(el, false)); settle(); };
    document.addEventListener('focusin', onFocus);
    document.addEventListener('visibilitychange', onVisibility);
    window.addEventListener('beforeprint', showAll);
    return () => {
      window.clearTimeout(stop);
      mo.disconnect();
      io.disconnect();
      host.removeEventListener('pointerdown', stopWatching, { capture: true });
      host.removeEventListener('keydown', stopWatching, { capture: true });
      document.removeEventListener('focusin', onFocus);
      document.removeEventListener('visibilitychange', onVisibility);
      window.removeEventListener('beforeprint', showAll);
      pending.forEach(el => el.classList.remove(WAITING));
      // 다시 열릴 때(전체 메뉴) 자체 CSS 등장이 돌아오게
      touched.forEach(el => { el.style.animation = ''; });
    };
  }, [root, selector, ready]);
}
