// 펼침 애니메이션 — '스마트링크 만들기'처럼 누르면 카드 안에 입력칸·안내가 새로 나타나는 곳은 높이가 0에서 자라며
// 펼쳐진다. 아래 내용도 한 번에 툭 밀리지 않고 같이 부드럽게 내려간다. 화면마다 따로 손대지 않도록
// 새로 붙은 요소를 지켜보다가, 사람이 막 누르거나 입력한 직후에 붙은 펼침 블록만 움직인다
// (처음 화면을 그릴 때나 데이터가 늦게 도착해 붙은 블록은 그대로).

/** 펼쳐지는 블록 — 누르면 나타나는 입력 폼·패널. 다른 곳은 data-expand를 붙이면 같이 움직인다 */
const EXPAND = [
  '.aq-reveal', '.aq-correction-form', '.aq-fix-panel', '.track-detail-body', '.aq-takedown-list',
  '.aq-tax-more', '.aq-report-note', '.aq-splits-row', '[data-expand]',
].join(',');
/** 누른 뒤 이 시간 안에 붙은 블록만 펼친다 (서버 응답 뒤 열리는 폼까지 포함) */
const AFTER_INPUT_MS = 900;
const EASE = 'cubic-bezier(.22,1,.36,1)';

let lastInput = -Infinity;
const reduced = () => !!window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;

/** 움직일 만한 블록인지 — 너무 작거나(한 줄 글자) 화면보다 훨씬 큰 블록은 움직이면 오히려 느리다 */
function movable(el: HTMLElement, h: number) {
  if (h < 12 || h > window.innerHeight * 1.4) return false;
  const cs = getComputedStyle(el);
  return cs.display !== 'inline' && cs.position !== 'absolute' && cs.position !== 'fixed';
}

/** 높이가 클수록 조금 더 길게, 그래도 0.38초를 넘지 않게 */
const durationFor = (h: number, base: number) => Math.round(Math.min(380, base + h * 0.18));

/** box-sizing: border-box라 높이에 안쪽 여백이 들어 있다 — 여백·바깥 여백도 0에서 함께 자라게(줄게) 한다 */
function frames(cs: CSSStyleDeclaration, h: number) {
  const closed = { height: '0px', paddingTop: '0px', paddingBottom: '0px', marginTop: '0px', marginBottom: '0px' };
  const open = { height: `${h}px`, paddingTop: cs.paddingTop, paddingBottom: cs.paddingBottom, marginTop: cs.marginTop, marginBottom: cs.marginBottom };
  return { closed, open };
}

function expand(el: HTMLElement) {
  const h = el.getBoundingClientRect().height;
  if (!movable(el, h)) return;
  const { closed, open } = frames(getComputedStyle(el), h);
  const prevOverflow = el.style.overflow;
  el.style.overflow = 'hidden';
  // 높이가 먼저 자라고 글자는 살짝 늦게 떠오른다 — 빈 칸이 먼저 열리고 내용이 채워지는 느낌
  const anim = el.animate([
    { ...closed, opacity: 0 },
    { opacity: 0, offset: 0.25 },
    { ...open, opacity: 1 },
  ], { duration: durationFor(h, 220), easing: EASE });
  const done = () => { el.style.overflow = prevOverflow; };
  anim.onfinish = done;
  anim.oncancel = done;
}

/** 닫힘 — 화면에서 이미 빠진 블록을 그 자리에 복사해 두고, 복사본이 접히며 사라진다.
 *  아래 내용이 한 번에 튀어 올라오지 않고 따라 올라온다. 복사본은 누를 수 없고 화면 읽기 프로그램도 건너뛴다. */
function collapse(removed: HTMLElement, parent: Node, next: Node | null) {
  if (!parent.isConnected || (next && next.parentNode !== parent)) return;
  const ghost = removed.cloneNode(true) as HTMLElement;
  ghost.setAttribute('aria-hidden', 'true');
  ghost.dataset.aqGhost = '';
  ghost.inert = true;
  ghost.style.pointerEvents = 'none';
  ghost.querySelectorAll('[id]').forEach(x => x.removeAttribute('id'));
  parent.insertBefore(ghost, next);
  const h = ghost.getBoundingClientRect().height;
  if (!movable(ghost, h)) { ghost.remove(); return; }
  const { closed, open } = frames(getComputedStyle(ghost), h);
  ghost.style.overflow = 'hidden';
  // 글자가 먼저 흐려지고 높이가 뒤따라 접힌다 — 펼칠 때의 거꾸로
  const anim = ghost.animate([
    { ...open, opacity: 1 },
    { opacity: 0, offset: 0.6 },
    { ...closed, opacity: 0 },
  ], { duration: durationFor(h, 200), easing: 'cubic-bezier(.4,0,.2,1)' });
  anim.onfinish = () => ghost.remove();
  anim.oncancel = () => ghost.remove();
}

export function initExpand(root: HTMLElement = document.body) {
  const mark = () => { lastInput = performance.now(); };
  document.addEventListener('pointerup', mark, { capture: true, passive: true });
  document.addEventListener('keydown', mark, { capture: true, passive: true });
  document.addEventListener('change', mark, { capture: true, passive: true });

  new MutationObserver(records => {
    if (reduced() || performance.now() - lastInput > AFTER_INPUT_MS) return;
    for (const r of records) {
      // 새로 붙은(빠진) 덩어리의 맨 위가 펼침 블록일 때만 — 카드나 탭 전체가 새로 그려질 땐 그 안을 따로 움직이지 않는다
      // 닫힘 복사본(data-aq-ghost)이 붙고 빠지는 것은 건너뛴다
      for (const n of r.addedNodes) {
        if (n instanceof HTMLElement && n.matches(EXPAND) && !n.hasAttribute('data-aq-ghost') && typeof n.animate === 'function') expand(n);
      }
      for (const n of r.removedNodes) {
        if (n instanceof HTMLElement && n.matches(EXPAND) && !n.hasAttribute('data-aq-ghost') && typeof n.animate === 'function') collapse(n, r.target, r.nextSibling);
      }
    }
  }).observe(root, { childList: true, subtree: true });
}
