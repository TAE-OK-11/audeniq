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

function expand(el: HTMLElement) {
  const h = el.getBoundingClientRect().height;
  // 너무 작거나(한 줄 글자) 화면보다 훨씬 큰 블록은 펼치면 오히려 느리다
  if (h < 12 || h > window.innerHeight * 1.4) return;
  const cs = getComputedStyle(el);
  if (cs.display === 'inline' || cs.position === 'absolute' || cs.position === 'fixed') return;
  const duration = Math.round(Math.min(380, 220 + h * 0.18));
  const prevOverflow = el.style.overflow;
  el.style.overflow = 'hidden';
  // box-sizing: border-box라 높이에 안쪽 여백이 들어 있다 — 여백도 0에서 함께 자라게 한다
  const anim = el.animate([
    { height: '0px', paddingTop: '0px', paddingBottom: '0px', marginTop: '0px', marginBottom: '0px', opacity: 0 },
    { height: `${h}px`, paddingTop: cs.paddingTop, paddingBottom: cs.paddingBottom, marginTop: cs.marginTop, marginBottom: cs.marginBottom, opacity: 1 },
  ], { duration, easing: EASE });
  const done = () => { el.style.overflow = prevOverflow; };
  anim.onfinish = done;
  anim.oncancel = done;
}

export function initExpand(root: HTMLElement = document.body) {
  const reduce = window.matchMedia?.('(prefers-reduced-motion: reduce)');
  const mark = () => { lastInput = performance.now(); };
  document.addEventListener('pointerup', mark, { capture: true, passive: true });
  document.addEventListener('keydown', mark, { capture: true, passive: true });
  document.addEventListener('change', mark, { capture: true, passive: true });

  new MutationObserver(records => {
    if (reduce?.matches || performance.now() - lastInput > AFTER_INPUT_MS) return;
    for (const r of records) {
      for (const n of r.addedNodes) {
        // 새로 붙은 덩어리의 맨 위가 펼침 블록일 때만 — 카드나 탭 전체가 새로 그려질 땐 그 안을 따로 펼치지 않는다
        if (n instanceof HTMLElement && n.matches(EXPAND) && typeof n.animate === 'function') expand(n);
      }
    }
  }).observe(root, { childList: true, subtree: true });
}
