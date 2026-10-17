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
// 아이폰과 같은 곡선 — 첫 프레임에 툭 튀지 않고 길게 감속한다 (tokens.css --aq-enter)
const EASE = 'cubic-bezier(.32,.72,0,1)';

let lastInput = -Infinity;
/** 마지막으로 사용자가 머문 스크롤 위치 — 닫기 처리 중에 다른 부품이 크기를 재면 그 순간 브라우저가 이미 스크롤을
 *  끌어올리므로 닫힌 뒤의 위치는 믿을 수 없다. 스크롤 이벤트는 그 다음 그림 차례에 오기 때문에, 여기 남은 값은
 *  끌어올리기 전 위치다 */
let lastY = 0;
/** 마지막으로 누른 곳 — '저장'처럼 서버 응답을 기다린 뒤 닫히는 폼도 누른 폼이 빠지는 것으로 알아본다 */
let lastTarget: Element | null = null;
/** 누른 폼이 응답을 기다렸다 닫히는 경우를 이만큼까지 기다려 준다 */
const AFTER_SAVE_MS = 15000;
const reduced = () => !!window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;

/** 움직일 만한 블록인지 — 너무 작거나(한 줄 글자) 화면보다 훨씬 큰 블록은 움직이면 오히려 느리다 */
function movable(el: HTMLElement, h: number) {
  if (h < 12 || h > window.innerHeight * 1.4) return false;
  const cs = getComputedStyle(el);
  return cs.display !== 'inline' && cs.position !== 'absolute' && cs.position !== 'fixed';
}

/** 높이가 클수록 조금 더 길게, 그래도 0.38초를 넘지 않게 */
const durationFor = (h: number, base: number) => Math.round(Math.min(380, base + h * 0.18));

/** 세로로 쌓는 목록(flex 세로·grid)의 줄 사이 간격 — 앞에 형제가 있는 블록만 이 간격을 차지한다 */
function gapBefore(el: HTMLElement) {
  const p = el.parentElement;
  if (!p || !el.previousElementSibling) return 0;
  const pcs = getComputedStyle(p);
  const stacked = pcs.display.includes('grid') || (pcs.display.includes('flex') && pcs.flexDirection.startsWith('column'));
  return stacked ? parseFloat(pcs.rowGap) || 0 : 0;
}

/** 목록 줄이면 줄 사이 간격도 함께 자라게(줄게) — 간격만 한 번에 생기거나 사라지며 아래 내용이 몇 px 툭 튀었다.
 *  grid 줄 높이는 0 밑으로 줄지 않아 블록 자신의 음수 여백은 듣지 않으므로, 앞 형제의 아래 여백을 간격만큼 당겨 둔다 */
function animateGap(el: HTMLElement, opening: boolean, timing: KeyframeAnimationOptions) {
  const gap = gapBefore(el);
  const prev = el.previousElementSibling as HTMLElement | null;
  if (!gap || !prev) return;
  const mb = getComputedStyle(prev).marginBottom;
  const shut = `calc(${mb} - ${gap}px)`;
  prev.animate([{ marginBottom: opening ? shut : mb }, { marginBottom: opening ? mb : shut }], timing);
}

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
  animateGap(el, true, { duration: durationFor(h, 220), easing: EASE });
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
  animateGap(ghost, false, { duration: durationFor(h, 200), easing: 'cubic-bezier(.4,0,.2,1)' });
  anim.onfinish = () => ghost.remove();
  anim.oncancel = () => ghost.remove();
}

/** 닫은 뒤 보던 자리 지키기 — 페이지 아래쪽에서 폼을 닫으면 페이지가 그만큼 짧아져 브라우저가 스크롤을 끌어올리고,
 *  보던 카드가 화면 아래로 밀려 "바닥으로 간" 것처럼 보였다. 화면(.view)의 높이를 닫기 직전 높이로 붙잡아 두어
 *  카드가 그 자리에 있게 하고, 사용자가 위로 올라가 붙잡은 높이가 필요 없어지면 그만큼씩 놓는다(다른 화면으로 가면 사라진다).
 *  y는 누를 때의 스크롤 위치 — 그사이 브라우저가 끌어올렸으면 그림이 그려지기 전에 되돌린다. */
function holdPlace(from: Node, y: number) {
  const el = from instanceof Element ? from : from.parentElement;
  const view = el?.closest<HTMLElement>('.view') ?? el?.closest<HTMLElement>('main') ?? null;
  if (!view) return;
  const doc = document.documentElement;
  /** 붙잡은 높이 없이 내용만의 높이 — 붙잡은 높이를 풀지 않고 잰다(풀었다가 재면 그 순간 스크롤이 끌려 올라간다) */
  const contentHeight = () => {
    const top = view.getBoundingClientRect().top;
    let bottom = top;
    for (const c of view.children) bottom = Math.max(bottom, c.getBoundingClientRect().bottom + parseFloat(getComputedStyle(c).marginBottom || '0'));
    return bottom - top + parseFloat(getComputedStyle(view).paddingBottom || '0');
  };
  /** 지금 스크롤 위치(at)를 지키는 데 필요한 화면 높이 — 내용이 충분하면 0 */
  const needed = (at: number) => {
    const extra = view.offsetHeight - contentHeight();
    const lack = at + window.innerHeight - (doc.scrollHeight - extra);
    return lack > 1 ? contentHeight() + lack : 0;
  };
  const apply = (at: number) => {
    const h = needed(at);
    if (h) { view.style.minHeight = `${Math.ceil(h)}px`; return true; }
    view.style.minHeight = '';
    return false;
  };
  if (!view.hasAttribute('data-aq-hold')) {
    view.setAttribute('data-aq-hold', '');
    // 위로 올라갈수록 필요한 높이가 줄어든다 — 필요한 만큼만 남기다가 더 필요 없으면 놓는다
    const release = () => {
      if (view.isConnected && apply(window.scrollY)) return;
      view.removeAttribute('data-aq-hold');
      window.removeEventListener('scroll', release);
    };
    window.addEventListener('scroll', release, { passive: true });
  }
  // 접히는 복사본이 아직 높이를 갖고 있어도, 다 접힌 뒤를 기준으로 붙잡아 둔다
  view.style.minHeight = `${Math.ceil(Math.max(view.offsetHeight, contentHeight() + Math.max(0, y + window.innerHeight - doc.scrollHeight)))}px`;
  if (window.scrollY !== y) window.scrollTo({ top: y, behavior: 'instant' as ScrollBehavior });
}

/** 펼칠 때 카드가 화면 위에서 얼마나 내려와 있었는지 — 펼친 폼을 끝까지 내려가 아래 '닫기'로 닫으면
 *  끝까지 스크롤된 자리에 남지 않고, 처음 카드를 연 그 자리(카드가 같은 높이에 보이는 곳)로 돌아간다 */
const openedAt = new WeakMap<Node, number>();

function rememberOpen(parent: Node) {
  if (parent instanceof Element) openedAt.set(parent, parent.getBoundingClientRect().top);
}

/** 연 자리보다 아래로 내려가 있으면 그 자리로 돌아간다. 돌아갔으면 true */
function returnToOpen(parent: Node, y: number) {
  const top = openedAt.get(parent);
  openedAt.delete(parent);
  if (top === undefined || !(parent instanceof Element) || !parent.isConnected) return false;
  // 지금 문서에서의 카드 위치 — 닫는 사이 브라우저가 스크롤을 끌어올렸어도 y(누를 때 위치) 기준으로 잰다
  const cardTop = window.scrollY + parent.getBoundingClientRect().top;
  const target = Math.max(0, cardTop - top);
  if (target >= y - 1) return false; // 연 자리보다 위에 있거나 그대로면 보던 자리를 지킨다
  if (window.scrollY !== y) window.scrollTo({ top: y, behavior: 'instant' as ScrollBehavior });
  window.scrollTo({ top: target, behavior: reduced() ? 'instant' as ScrollBehavior : 'smooth' });
  return true;
}

/** 누른 버튼이 폼과 함께 사라지면 커서(포커스)가 페이지 맨 처음으로 날아간다 — 같은 카드의 첫 버튼으로 돌려놓는다 */
let lastFocused: Element | null = null;
function restoreFocus(removed: Element, parent: Node) {
  if (!lastFocused || lastFocused.isConnected || !removed.contains(lastFocused)) return;
  if (document.activeElement && document.activeElement !== document.body) return;
  const card = (parent instanceof Element ? parent : parent.parentElement)?.closest<HTMLElement>('section,.ui-card,.aq-manage-card,.aq-set-card,.ui-row');
  const target = card ? [...card.querySelectorAll<HTMLElement>('button:not(:disabled),a[href],input,select,textarea')].find(x => !x.closest('[data-aq-ghost]')) : null;
  target?.focus({ preventScroll: true });
}

export function initExpand(root: HTMLElement = document.body) {
  const mark = (e: Event) => { lastInput = performance.now(); lastTarget = e.target instanceof Element ? e.target : null; };
  lastY = window.scrollY;
  window.addEventListener('scroll', () => { lastY = window.scrollY; }, { passive: true });
  document.addEventListener('pointerup', mark, { capture: true, passive: true });
  document.addEventListener('keydown', mark, { capture: true, passive: true });
  document.addEventListener('change', mark, { capture: true, passive: true });
  document.addEventListener('focusin', e => { lastFocused = e.target instanceof Element ? e.target : null; }, true);

  new MutationObserver(records => {
    const since = performance.now() - lastInput;
    if (since > AFTER_SAVE_MS) return;
    // 바로 닫힌 것, 또는 누른 폼이 저장을 기다렸다 닫힌 것만
    const userRemoved = (n: Node): n is HTMLElement => n instanceof HTMLElement && n.matches(EXPAND) && !n.hasAttribute('data-aq-ghost')
      && (since <= AFTER_INPUT_MS || (!!lastTarget && n.contains(lastTarget)));
    if (since > AFTER_INPUT_MS && !records.some(r => [...r.removedNodes].some(userRemoved))) return;
    const y = lastY;
    const userAdded = (n: Node): n is HTMLElement => since <= AFTER_INPUT_MS && n instanceof HTMLElement && n.matches(EXPAND) && !n.hasAttribute('data-aq-ghost');
    // 누른 직후 붙은 펼침 블록: 카드를 연 자리를 기억한다 / 빠진 블록: 커서를 돌려놓는다 (동작 줄이기 설정이어도)
    for (const r of records) {
      if ([...r.addedNodes].some(userAdded)) rememberOpen(r.target);
      for (const n of r.removedNodes) if (userRemoved(n)) restoreFocus(n, r.target);
    }
    if (reduced()) {
      const r0 = records.find(r => [...r.removedNodes].some(userRemoved));
      if (r0 && !returnToOpen(r0.target, y)) holdPlace(r0.target, y);
      return;
    }
    let held = false;
    for (const r of records) {
      // 새로 붙은(빠진) 덩어리의 맨 위가 펼침 블록일 때만 — 카드나 탭 전체가 새로 그려질 땐 그 안을 따로 움직이지 않는다
      // 닫힘 복사본(data-aq-ghost)이 붙고 빠지는 것은 건너뛴다
      for (const n of r.addedNodes) {
        if (userAdded(n) && typeof n.animate === 'function') expand(n);
      }
      for (const n of r.removedNodes) {
        if (userRemoved(n) && typeof n.animate === 'function') {
          collapse(n, r.target, r.nextSibling);
          // 연 자리보다 내려와 있으면 그 자리로 돌아가고, 아니면 접히는 동안에도 페이지가 짧아지지 않게 지금 높이를 붙잡는다
          if (!held) { if (!returnToOpen(r.target, y)) holdPlace(r.target, y); held = true; }
        }
      }
    }
  }).observe(root, { childList: true, subtree: true });
}
