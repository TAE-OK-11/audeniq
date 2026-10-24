// 화면 등장 연출 — audeniq.com 랜딩과 같은 움직임을 모든 화면에 (홈은 useLandingReveal, 발매 신청은 자체 단계 모션).
// 머리(작은 제목 → 제목 → 설명 → 버튼)가 몇십 ms씩 차례로 16px 아래에서 떠오르고, 이어서 카드·목록 줄·섹션 머리가 나온다.
// 처음부터 화면 안에 있던 덩어리는 0.6초 동안 차례로(앱을 연 첫 화면 — 화면을 옮겨 다닐 때는 0.36초로 짧게, entryMotion),
// 화면 밖 덩어리는 스크롤로 닿기 조금 전에 시작해 0.48초.
// 늦게 그려지는 목록(불러오기 뒤)도 잠시 지켜보다 같은 방식으로 맡는다. 바깥 덩어리를 맡으면 그 안쪽은 따로 움직이지 않는다.
// 사용자가 화면을 누르거나 키를 치면 지켜보기를 그만둔다 — 탭·필터로 바뀐 내용까지 맡으면 한 번 사라졌다가
// 다시 떠올라 튀어 보였다(탭 패널은 자체 전환이 있다). 그 뒤로 바뀌는 내용은 바로 보인다.
// 초점이 들어가면 바로 제자리, 탭을 숨기면 진행 중인 연출을 끝낸다. 동작 줄이기·미지원 브라우저에서는 아무것도 하지 않는다.
// 숨기기는 그리기 전에(useLayoutEffect·MutationObserver) — 한 프레임이라도 보였다가 사라지지 않게.
import { useLayoutEffect } from 'react';

/** 머리 — 차례로 */
const HEAD: [string, number][] = [
  ['.ui-page-head .eyebrow', 0],
  ['.ui-page-head h1', 50],
  ['.ui-page-head h1 ~ p, .ui-page-head h1 ~ div', 100],
  ['.ui-page-head > :not(:first-child)', 140],
];
/** 덩어리 — 카드, 목록 줄, 섹션 머리, 안내 상자, 탭 */
const UNITS = [
  '.ui-card', '.ui-list > *', '.ui-row', '.ui-section-head', '.ui-callout', '.ui-empty', '.ui-tabs', '.ui-hero',
  '.aq-bundle', '.aq-faq-item', '.aq-nboard-row', '.aq-nhero', '.aq-event-hero', '.settle-hero', '.aq-detail-hero',
  '.surface', '.aq-report-card', '.aq-rp-card', '.aq-profile-card', '.aq-doc-list', '.aq-settings > *',
].join(',');

/** 아래에서 떠오르는 등장 */
export const rise = (px: number): Keyframe[] => [{ opacity: 0, transform: `translateY(${px}px)` }, { opacity: 1, transform: 'none' }];
const EASING = 'cubic-bezier(.22,.61,.36,1)';
const SCROLL_MS = 480;
const SCROLL_RISE = rise(16);

/** 화면 첫 등장의 크기 — 앱을 연 뒤 첫 화면은 랜딩처럼 길게(0.6초·16px), 그 뒤 탭·메뉴로 화면을 옮길 때는 짧고 작게
 * (0.36초·10px, 차례 간격도 절반쯤). 자주 오가는 화면마다 0.6초 계단을 다시 보면 느리고 꾸민 티가 났다.
 * 처음 부른 뒤 잠깐(개발 모드에서 효과가 두 번 도는 사이) 안의 호출도 첫 화면으로 친다. */
export interface EntryMotion { ms: number; keyframes: Keyframe[]; pace: number }
const FULL_ENTRY: EntryMotion = { ms: 600, keyframes: rise(16), pace: 1 };
const NAV_ENTRY: EntryMotion = { ms: 360, keyframes: rise(10), pace: 0.6 };
let firstEntryAt = -1;
export function entryMotion(): EntryMotion {
  const now = performance.now();
  if (firstEntryAt < 0) firstEntryAt = now;
  return now - firstEntryAt < 400 ? FULL_ENTRY : NAV_ENTRY;
}
const WAITING = 'aq-motion-waiting';
/** 화면을 연 뒤 이 시간 안에 화면 안에 그려진 덩어리는 첫 화면 흐름(차례)을 탄다 */
const FIRST_WINDOW = 1500;
const WATCH_MS = 4000;

/** 한 번 재생되는 CSS 등장 애니메이션이 걸려 있는지 (반복 애니메이션 — 스켈레톤·스피너 — 은 그대로 둔다) */
export function hasEntranceAnimation(el: HTMLElement): boolean {
  const cs = getComputedStyle(el);
  return cs.animationName !== 'none' && !cs.animationIterationCount.split(',').some(v => v.trim() === 'infinite');
}

export function useScrollReveal(key: string) {
  useLayoutEffect(() => {
    const reduced = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;
    if (reduced || typeof IntersectionObserver === 'undefined' || !Element.prototype.animate) return;
    const root = document.getElementById('main');
    if (!root) return;
    const started = performance.now();
    const entry = entryMotion();
    const seen = new WeakSet<Element>();
    const pending = new Set<HTMLElement>();
    const active = new Map<HTMLElement, Animation>();
    const meta = new Map<HTMLElement, { delay: number; first: boolean }>();
    let firstCount = 0;

    const show = (el: HTMLElement, now = false) => {
      if (!pending.delete(el)) return;
      io.unobserve(el);
      const m = meta.get(el);
      if (!now && !document.hidden && m) {
        const a = el.animate(m.first ? entry.keyframes : SCROLL_RISE, { duration: m.first ? entry.ms : SCROLL_MS, easing: EASING, fill: 'backwards', delay: m.delay });
        active.set(el, a);
        a.finished.then(() => { if (active.get(el) === a) active.delete(el); }, () => {});
      }
      el.classList.remove(WAITING);
    };
    const io = new IntersectionObserver(entries => {
      entries.forEach(e => { if (e.isIntersecting) show(e.target as HTMLElement); });
    }, { threshold: 0, rootMargin: '0px 0px 64px 0px' });

    // 읽기(위치)를 모두 끝낸 뒤 쓰기(숨김 클래스)를 한다 — 섞으면 요소마다 레이아웃을 다시 계산해 화면 진입이 멈칫한다
    const takeAll = (list: [HTMLElement, number | undefined][]) => {
      const fresh = list.filter(([el]) => {
        if (seen.has(el)) return false;
        seen.add(el);
        return !el.contains(document.activeElement);
      });
      const now = performance.now() - started < FIRST_WINDOW && window.scrollY < 40;
      const vh = window.innerHeight;
      const rects = fresh.map(([el]) => el.getBoundingClientRect());
      const cssEntrance = fresh.map(([el]) => hasEntranceAnimation(el));
      fresh.forEach(([el, headDelay], i) => {
        const r = rects[i];
        if (!r.width && !r.height) return; // 숨겨진 것
        const first = now && r.top < vh;
        // 첫 화면: 머리 다음에 덩어리를 60ms씩, 네 개마다 다시 (길게 늘어지지 않게)
        const delay = Math.round((headDelay ?? (first ? 150 + (firstCount++ % 4) * 60 : 0)) * entry.pace);
        meta.set(el, { delay, first });
        // 자체 CSS 등장 애니메이션이 있으면 끈다 — 둘이 겹치면 CSS로 먼저 다 나타났다가 이 연출이 다시 숨겼다
        if (cssEntrance[i]) el.style.animation = 'none';
        pending.add(el);
        el.classList.add(WAITING);
        // 이미 화면 안이면 보임 신호를 기다리지 않고 바로 시작한다 (아이폰은 당겨서 새로고침 직후 등 바쁠 때 신호가 0.4초쯤 늦었다)
        if (r.top < vh && r.bottom > 0) show(el);
        else io.observe(el);
      });
    };

    const scan = () => {
      const view = root.querySelector<HTMLElement>(':scope .view');
      // 홈은 자체 연출, 발매 신청은 단계 모션
      if (!view || view.id === 'view-home' || view.id === 'view-new') return;
      const list: [HTMLElement, number | undefined][] = [];
      for (const [sel, d] of HEAD) view.querySelectorAll<HTMLElement>(sel).forEach(el => list.push([el, d]));
      const vh = window.innerHeight;
      view.querySelectorAll<HTMLElement>(UNITS).forEach(el => {
        if (el.closest('[data-no-reveal], .ui-page-head')) return;
        // 바깥 덩어리를 맡았으면 안쪽은 따로 움직이지 않는다 — 다만 바깥이 화면 아래까지 길게 이어지면
        // (발매 목록·정산 내역처럼 줄이 많은 카드) 화면 밖에 있던 안쪽 줄은 스크롤로 닿을 때 따로 떠오른다.
        // 그러지 않으면 첫 화면만 움직이고 스크롤해 올라오는 줄은 처음부터 고정돼 있었다
        const outer = el.parentElement?.closest<HTMLElement>(UNITS);
        if (outer && (el.getBoundingClientRect().top < vh || outer.getBoundingClientRect().top >= vh)) return;
        list.push([el, undefined]);
      });
      // 덩어리가 하나도 없는 화면 조각(글 본문 등)은 그 조각을 통째로
      Array.from(view.children).forEach(c => {
        const el = c as HTMLElement;
        if (el.matches('.ui-page-head') || el.matches(UNITS) || el.querySelector(UNITS)) return;
        list.push([el, undefined]);
      });
      takeAll(list);
      // 처음 그려진 탭 내용은 안쪽 덩어리가 떠오르므로 탭 내용 자체의 등장(옅게 6px)은 끈다 — 둘이 겹쳐 두 번 움직였다.
      // 탭을 바꿀 때 새로 붙는 내용(누른 쪽에서 들어오는 연출)은 그대로 둔다
      if (performance.now() - started < FIRST_WINDOW) {
        view.querySelectorAll<HTMLElement>('.ui-tab-panel:not(.is-from-right):not(.is-from-left)').forEach(el => { el.style.animation = 'none'; });
      }
    };

    scan();
    // 늦게 그려지는 목록·카드도 맡는다 — 바뀐 내용은 그리기 전에(마이크로태스크) 훑는다
    const mo = new MutationObserver(scan);
    mo.observe(root, { childList: true, subtree: true });
    const stopWatching = () => mo.disconnect();
    const stop = window.setTimeout(stopWatching, WATCH_MS);
    root.addEventListener('pointerdown', stopWatching, { capture: true, once: true });
    root.addEventListener('keydown', stopWatching, { capture: true, once: true });

    const settle = () => { active.forEach(a => a.cancel()); active.clear(); };
    const onFocus = (e: FocusEvent) => {
      const t = e.target as Node;
      [...pending].forEach(el => { if (el.contains(t)) show(el, true); });
      active.forEach((a, el) => { if (el.contains(t)) { a.cancel(); active.delete(el); } });
    };
    const onVisibility = () => { if (document.hidden) settle(); };
    const showAll = () => { [...pending].forEach(el => show(el, true)); settle(); };
    document.addEventListener('focusin', onFocus);
    document.addEventListener('visibilitychange', onVisibility);
    window.addEventListener('beforeprint', showAll);

    return () => {
      window.clearTimeout(stop);
      mo.disconnect();
      root.removeEventListener('pointerdown', stopWatching, { capture: true });
      root.removeEventListener('keydown', stopWatching, { capture: true });
      io.disconnect();
      document.removeEventListener('focusin', onFocus);
      document.removeEventListener('visibilitychange', onVisibility);
      window.removeEventListener('beforeprint', showAll);
      pending.forEach(el => el.classList.remove(WAITING));
    };
  }, [key]);
}
