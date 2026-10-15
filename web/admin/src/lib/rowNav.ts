// 탭·칩 줄 상호작용 — 화면마다 따로 만든 줄까지 한 번에.
// 1) 가로로 넘치는 탭·칩 줄에서 고른 칸이 가장자리에 걸리거나 화면 밖에 있으면 보이는 자리로 부드럽게 민다
//    (휴대폰에서 발매 상세 탭 6개, 발매 상태 칩처럼 오른쪽이 잘린 줄).
// 2) 탭 목록·고르기 토글에서 ← → Home End로 옮겨 고른다 (WAI-ARIA tablist·radiogroup 키보드 규칙).

const ITEM = '.ui-tab,.ui-chip,.ui-seg>button,[role=tab],[role=radio]';
const SELECTED = '[aria-selected="true"],[aria-pressed="true"],[aria-checked="true"],.is-on,.active';
const EDGE = 24;

/** 가로로 넘치는 가장 가까운 조상 (줄 자체이거나 그 바로 위) */
function scroller(el: HTMLElement): HTMLElement | null {
  let p = el.parentElement;
  for (let i = 0; p && i < 3; i++, p = p.parentElement) {
    const ox = getComputedStyle(p).overflowX;
    if ((ox === 'auto' || ox === 'scroll') && p.scrollWidth > p.clientWidth + 1) return p;
  }
  return null;
}

/** el이 줄 안에서 다 보이도록 가로로만 민다 (세로 스크롤은 건드리지 않는다) */
export function revealInRow(el: HTMLElement, smooth = true) {
  const box = scroller(el);
  if (!box) return;
  const b = box.getBoundingClientRect();
  const r = el.getBoundingClientRect();
  let dx = 0;
  if (r.left < b.left + EDGE) dx = r.left - b.left - EDGE;
  else if (r.right > b.right - EDGE) dx = r.right - b.right + EDGE;
  if (Math.abs(dx) < 1) return;
  const reduce = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;
  box.scrollTo({ left: box.scrollLeft + dx, behavior: smooth && !reduce ? 'smooth' : 'auto' });
}

/** 새로 그려진 줄에서 이미 골라져 있는 칸이 줄 밖이면 한 번만 보이게 (주소로 바로 들어온 탭 등).
 *  사용자가 줄을 직접 민 뒤에는 다시 끌어오지 않도록 줄마다 한 번만 한다. */
const seen = new WeakSet<Element>();
function revealSelected(root: ParentNode) {
  root.querySelectorAll<HTMLElement>('.ui-tabs,.ui-chips,.ui-seg,[role=tablist]').forEach(row => {
    if (seen.has(row)) return;
    const on = row.querySelector<HTMLElement>(SELECTED);
    if (!on) return; // 아직 불러오는 중인 줄은 고른 칸이 생긴 뒤에
    seen.add(row);
    revealInRow(on, false);
  });
}

/** 옆으로 넘치는 줄 — 더 밀 수 있는 쪽 끝을 흐리게 (aq-more-left / aq-more-right). 끝까지 밀면 그쪽 흐림이 사라진다 */
const ROWS = '.ui-tabs,.ui-chips,.ui-seg,[role=tablist]';
function fadeTarget(row: HTMLElement): HTMLElement | null {
  const ox = getComputedStyle(row).overflowX;
  if (ox === 'auto' || ox === 'scroll') return row;
  const first = row.firstElementChild;
  return first instanceof HTMLElement ? scroller(first) : null;
}
function updateFade(box: HTMLElement) {
  const max = box.scrollWidth - box.clientWidth;
  box.classList.toggle('aq-more-left', max > 1 && box.scrollLeft > 4);
  box.classList.toggle('aq-more-right', max > 1 && box.scrollLeft < max - 4);
}
/** 끝 흐림을 다는 줄 — 밀 때(scroll) 이 줄만 다시 잰다 */
const fadeBoxes = new WeakSet<Element>();
function updateFades(root: ParentNode) {
  root.querySelectorAll<HTMLElement>(ROWS).forEach(row => {
    const box = fadeTarget(row);
    if (!box) return;
    fadeBoxes.add(box);
    updateFade(box);
  });
}

export function initRowNav() {
  document.addEventListener('click', e => {
    const t = e.target instanceof Element ? e.target.closest<HTMLElement>(ITEM) : null;
    // 클릭으로 화면이 다시 그려진 뒤에 잰다
    if (t) requestAnimationFrame(() => { if (t.isConnected) revealInRow(t); });
  });

  document.addEventListener('keydown', e => {
    if (e.altKey || e.ctrlKey || e.metaKey) return;
    const t = e.target instanceof HTMLElement ? e.target : null;
    if (!t?.matches('[role=tab],[role=radio]')) return;
    const group = t.closest('[role=tablist],[role=radiogroup]');
    if (!group) return;
    const items = [...group.querySelectorAll<HTMLElement>(t.matches('[role=tab]') ? '[role=tab]' : '[role=radio]')]
      .filter(x => !x.matches(':disabled,[aria-disabled=true]') && x.offsetParent !== null);
    const i = items.indexOf(t);
    if (i < 0) return;
    const next = e.key === 'ArrowRight' || e.key === 'ArrowDown' ? items[(i + 1) % items.length]
      : e.key === 'ArrowLeft' || e.key === 'ArrowUp' ? items[(i - 1 + items.length) % items.length]
        : e.key === 'Home' ? items[0] : e.key === 'End' ? items[items.length - 1] : null;
    if (!next || next === t) return;
    e.preventDefault();
    next.focus();
    next.click();
  });

  // 화면이 바뀌어 새 탭·칩 줄이 그려지면 고른 칸을 보이게 한다
  let pending = 0;
  new MutationObserver(() => {
    if (pending) return;
    pending = requestAnimationFrame(() => { pending = 0; revealSelected(document); updateFades(document); });
  }).observe(document.getElementById('root') ?? document.body, { childList: true, subtree: true });

  // 탭을 바꾸면 누른 탭이 있는 쪽에서 내용이 들어온다 (오른쪽 탭 → 오른쪽에서, 왼쪽 탭 → 왼쪽에서).
  // 누르기 직전(capture)에 지금 고른 칸과 누른 칸의 순서를 재 두고, 새로 붙는 탭 내용(.ui-tab-panel)에 방향을 단다.
  // 새 내용이 그려지기 전(같은 작업 안의 microtask)에 달기 때문에 애니메이션이 처음부터 그 방향으로 시작한다.
  const TAB = '[role=tab],.ui-tab,.ui-seg>button';
  let tabDir: { dir: 1 | -1; at: number; list: HTMLElement | null } | null = null;
  document.addEventListener('click', e => {
    const t = e.target instanceof Element ? e.target.closest<HTMLElement>(TAB) : null;
    const items = t?.parentElement ? [...t.parentElement.children].filter(x => x.matches(TAB)) : [];
    const from = items.findIndex(x => x.matches(SELECTED));
    const to = t ? items.indexOf(t) : -1;
    tabDir = from >= 0 && to >= 0 && from !== to ? { dir: to > from ? 1 : -1, at: performance.now(), list: t?.parentElement ?? null } : null;
  }, { capture: true });
  new MutationObserver(records => {
    if (!tabDir || performance.now() - tabDir.at > 600) return;
    for (const r of records) {
      for (const n of r.addedNodes) {
        if (!(n instanceof HTMLElement)) continue;
        const panel = n.matches('.ui-tab-panel') ? n : n.querySelector<HTMLElement>(':scope > .ui-tab-panel');
        if (!panel) continue;
        panel.classList.add(tabDir.dir > 0 ? 'is-from-right' : 'is-from-left');
        // 아래까지 내려 읽다가 탭을 바꾸면 새 내용의 중간·끝(짧은 탭이면 엉뚱한 자리)이 보였다 —
        // 탭 줄이 화면 위로 지나가 있으면 탭 줄이 머리 바로 아래에 오도록 올려 새 내용을 처음부터 보여 준다
        const list = tabDir.list;
        tabDir = null;
        requestAnimationFrame(() => {
          if (!list?.isConnected) return;
          const head = (document.querySelector('.portal-header, .adm-header') as HTMLElement | null)?.offsetHeight ?? 64;
          const top = list.getBoundingClientRect().top;
          if (top >= head) return;
          const reduce = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;
          window.scrollTo({ top: window.scrollY + top - head - 8, behavior: reduce ? 'auto' : 'smooth' });
        });
        return;
      }
    }
  }).observe(document.getElementById('root') ?? document.body, { childList: true, subtree: true });

  // 줄을 밀거나 창 크기가 바뀌면 끝 흐림을 다시 잰다
  document.addEventListener('scroll', e => {
    const t = e.target;
    if (t instanceof HTMLElement && fadeBoxes.has(t)) updateFade(t);
  }, { capture: true, passive: true });
  window.addEventListener('resize', () => updateFades(document), { passive: true });
}
