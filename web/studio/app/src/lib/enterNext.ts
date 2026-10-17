// 키보드의 '다음' — 폼(<form>) 밖의 입력 화면(발매 신청 등)에서 한 줄 칸에 Enter를 누르면 아무 일도 없었다.
// 다음 칸이 있으면 키보드 키를 '다음'으로 보여 주고 Enter로 그 칸에 커서를 옮긴다. 마지막 칸이면 '완료'로 키보드를 내린다.
// <form> 안의 칸은 Enter가 제출이므로 건드리지 않는다(로그인·가입은 nextOnEnter로 따로 처리). 칸이 직접 Enter를 처리하면
// (preventDefault) 그대로 둔다. 한글 조합 중의 Enter(글자 확정)도 건드리지 않는다. 막고 싶은 칸은 data-no-enter-next.

const TEXT = 'input:not([type]),input[type=text],input[type=email],input[type=tel],input[type=url],input[type=number],input[type=password]';
const NEXT = `${TEXT},textarea`;
const SCOPE = '.aq-picker-box,.modal-inner,#view-new,#main';

function isTarget(el: EventTarget | null): el is HTMLInputElement {
  return el instanceof HTMLInputElement && el.matches(TEXT) && !el.closest('form,[data-no-enter-next]')
    && el.getAttribute('role') !== 'combobox' && !el.hasAttribute('aria-autocomplete');
}

function nextField(el: HTMLElement): HTMLElement | null {
  const scope = el.closest(SCOPE) ?? document.body;
  const all = Array.from(scope.querySelectorAll<HTMLElement>(NEXT)).filter(f => {
    if ((f as HTMLInputElement).disabled || (f as HTMLInputElement).readOnly || f.closest('[inert],[hidden],[data-no-enter-next]')) return false;
    const r = f.getBoundingClientRect();
    return r.width > 0 && r.height > 0 && getComputedStyle(f).visibility !== 'hidden';
  });
  return all[all.indexOf(el) + 1] ?? null;
}

export function initEnterNext(root: Document = document) {
  // 칸에 들어갈 때 키보드 키 이름을 정한다 — 칸이 직접 정해 둔 enterkeyhint는 따른다
  root.addEventListener('focusin', e => {
    const el = e.target;
    if (!isTarget(el) || el.hasAttribute('enterkeyhint')) return;
    el.setAttribute('enterkeyhint', nextField(el) ? 'next' : 'done');
    // 칸이 생기거나 사라져 다음 칸이 바뀌어도 다음 번 들어올 때 다시 정하게
    el.addEventListener('blur', () => el.removeAttribute('enterkeyhint'), { once: true });
  });
  root.addEventListener('keydown', e => {
    if (e.key !== 'Enter' || e.defaultPrevented || e.isComposing || e.keyCode === 229 || e.shiftKey || e.altKey || e.metaKey || e.ctrlKey) return;
    const el = e.target;
    if (!isTarget(el)) return;
    e.preventDefault();
    const next = nextField(el);
    if (next) next.focus();
    else el.blur();
  });
}
