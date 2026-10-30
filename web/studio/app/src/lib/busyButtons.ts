// 처리 중 버튼 표시 — 많은 버튼이 disabled로 막고 글자만 '저장하는 중'으로 바꾼다(is-busy를 따로 붙이지 않는다).
// 막힌 .ui-btn의 글자가 '…중'으로 끝나면 data-busy를 붙여, 회색 '못 누르는 버튼' 대신 점 세 개가 움직이게 한다(ui.css).
// 누르거나 폼을 보낼 때 그 버튼의 너비를 --busy-w로 적어 두어, 글자가 바뀌어도 버튼 너비가 그대로다.
const BUSY_TEXT = /중(…|\.\.\.)?$/;

function mark(btn: HTMLButtonElement) {
  const busy = btn.disabled && BUSY_TEXT.test((btn.textContent ?? '').trim());
  if (busy !== btn.hasAttribute('data-busy')) btn.toggleAttribute('data-busy', busy);
}

const remember = (btn: Element | null | undefined) => {
  if (btn instanceof HTMLElement && btn.matches('.ui-btn:not(.is-icon)')) btn.style.setProperty('--busy-w', `${btn.offsetWidth}px`);
};

/** 화면 전체의 버튼을 지켜본다. 돌려준 함수로 멈춘다 */
export function watchBusyButtons(root: HTMLElement = document.body): () => void {
  root.querySelectorAll<HTMLButtonElement>('button.ui-btn').forEach(mark);
  const onPress = (e: Event) => remember((e.target as Element | null)?.closest?.('.ui-btn'));
  const onKey = (e: KeyboardEvent) => { if (e.key === 'Enter' || e.key === ' ') onPress(e); };
  const onSubmit = (e: SubmitEvent) => remember(e.submitter ?? (e.target as HTMLFormElement).querySelector('button[type=submit].ui-btn'));
  root.addEventListener('pointerdown', onPress, true);
  root.addEventListener('keydown', onKey, true);
  root.addEventListener('submit', onSubmit, true);
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
  return () => {
    obs.disconnect();
    root.removeEventListener('pointerdown', onPress, true);
    root.removeEventListener('keydown', onKey, true);
    root.removeEventListener('submit', onSubmit, true);
  };
}
