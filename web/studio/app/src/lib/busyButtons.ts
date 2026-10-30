// 처리 중 버튼 표시 — 많은 버튼이 disabled로 막고 글자만 '저장하는 중'으로 바꾼다(is-busy를 따로 붙이지 않는다).
// 막힌 .ui-btn의 글자가 '…중'으로 끝나면 data-busy를 붙여, 회색 '못 누르는 버튼' 대신 처리 중 색이 되게 한다(ui.css).
const BUSY_TEXT = /중(…|\.\.\.)?$/;

function mark(btn: HTMLButtonElement) {
  const busy = btn.disabled && BUSY_TEXT.test((btn.textContent ?? '').trim());
  if (busy !== btn.hasAttribute('data-busy')) btn.toggleAttribute('data-busy', busy);
}

/** 화면 전체의 버튼을 지켜본다. 돌려준 함수로 멈춘다 */
export function watchBusyButtons(root: HTMLElement = document.body): () => void {
  root.querySelectorAll<HTMLButtonElement>('button.ui-btn').forEach(mark);
  const obs = new MutationObserver(list => {
    const seen = new Set<HTMLButtonElement>();
    for (const m of list) {
      const el = m.target.nodeType === Node.ELEMENT_NODE ? (m.target as Element) : m.target.parentElement;
      const btn = el?.closest<HTMLButtonElement>('button.ui-btn');
      if (btn) seen.add(btn);
      for (const n of m.addedNodes) {
        if (n.nodeType !== Node.ELEMENT_NODE) continue;
        const e = n as Element;
        if (e.matches('button.ui-btn')) seen.add(e as HTMLButtonElement);
        e.querySelectorAll<HTMLButtonElement>('button.ui-btn').forEach(b => seen.add(b));
      }
    }
    seen.forEach(mark);
  });
  obs.observe(root, { subtree: true, childList: true, characterData: true, attributes: true, attributeFilter: ['disabled'] });
  return () => obs.disconnect();
}
