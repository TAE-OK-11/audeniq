import { createContext, useCallback, useContext, useEffect, useId, useRef, useState, type ReactNode } from 'react';

interface ModalProps {
  title: string;
  onClose: () => void;
  children: ReactNode;
  /** #modal에 추가할 클래스 (예: aq-payout-setup-mode, aq-signature-mode) */
  modalClass?: string;
  /** 배경 클릭으로 닫기 (입력 중 실수로 닫히는 것을 막고 싶으면 false) */
  dismissible?: boolean;
}

const FOCUSABLE = 'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';
const EXIT_MS = 180;

// body 스크롤 잠금 카운터 — 중첩 모달에서 하나가 닫혀도 잠금 유지
let lockCount = 0;
let savedPadding = '';
function lockScroll() {
  lockCount += 1;
  if (lockCount === 1) {
    // 스크롤바가 사라지며 레이아웃이 밀리는 것을 방지
    const gap = window.innerWidth - document.documentElement.clientWidth;
    savedPadding = document.body.style.paddingRight;
    if (gap > 0) document.body.style.paddingRight = `${gap}px`;
    document.body.style.overflow = 'hidden';
  }
}
function unlockScroll() {
  lockCount = Math.max(0, lockCount - 1);
  if (lockCount === 0) {
    document.body.style.overflow = '';
    document.body.style.paddingRight = savedPadding;
  }
}

/** 모달 안의 자식이 닫힘 애니메이션과 함께 모달을 닫을 수 있게 한다 */
const ModalCloseContext = createContext<() => void>(() => {});
export const useModalClose = () => useContext(ModalCloseContext);

// 가장 위의 모달만 Esc/Tab에 반응하도록 스택 관리
const stack: symbol[] = [];

export function Modal({ title, onClose, children, modalClass, dismissible = true }: ModalProps) {
  const innerRef = useRef<HTMLElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const [leaving, setLeaving] = useState(false);
  const leavingRef = useRef(false);
  const titleId = useId();

  const requestClose = useCallback(() => {
    if (leavingRef.current) return;
    leavingRef.current = true;
    if (window.matchMedia?.('(prefers-reduced-motion: reduce)').matches) {
      onCloseRef.current();
      return;
    }
    setLeaving(true);
    window.setTimeout(() => onCloseRef.current(), EXIT_MS);
  }, []);

  useEffect(() => {
    const token = Symbol('modal');
    stack.push(token);
    const onKey = (e: KeyboardEvent) => {
      if (stack[stack.length - 1] !== token) return;
      if (e.key === 'Escape') {
        e.stopPropagation();
        requestClose();
        return;
      }
      // 포커스 트랩: Tab이 모달 안에서만 순환
      if (e.key === 'Tab') {
        const root = innerRef.current;
        if (!root) return;
        const list = Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE))
          .filter(el => el.offsetParent !== null);
        if (list.length === 0) { e.preventDefault(); return; }
        const first = list[0];
        const last = list[list.length - 1];
        if (e.shiftKey && (document.activeElement === first || !root.contains(document.activeElement))) {
          e.preventDefault();
          last.focus();
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault();
          first.focus();
        }
      }
    };
    document.addEventListener('keydown', onKey);
    lockScroll();
    // 열릴 때 모달 컨테이너로 포커스 이동(스크린리더가 제목부터 읽도록), 닫힐 때 복원
    const prevFocus = document.activeElement as HTMLElement | null;
    const t = window.setTimeout(() => {
      const root = innerRef.current;
      const auto = root?.querySelector<HTMLElement>('[autofocus], [data-autofocus]');
      (auto ?? root)?.focus({ preventScroll: true });
    }, 0);
    return () => {
      window.clearTimeout(t);
      document.removeEventListener('keydown', onKey);
      const i = stack.indexOf(token);
      if (i >= 0) stack.splice(i, 1);
      unlockScroll();
      if (prevFocus && document.contains(prevFocus)) prevFocus.focus({ preventScroll: true });
    };
  }, [requestClose]);

  const cls = ['modal', modalClass, leaving ? 'is-leaving' : ''].filter(Boolean).join(' ');

  return (
    <div
      id="modal"
      className={cls}
      onMouseDown={e => {
        if (dismissible && e.target === e.currentTarget) requestClose();
      }}
    >
      <section
        className="modal-inner" ref={innerRef} tabIndex={-1}
        role="dialog" aria-modal="true" aria-labelledby={titleId}
      >
        <div className="modal-top">
          <h2 id={titleId}>{title}</h2>
          <button className="icon-button" type="button" aria-label="닫기" onClick={requestClose}>
            <svg viewBox="0 0 24 24" width="20" height="20" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round"><path d="M6 6l12 12M18 6 6 18" /></svg>
          </button>
        </div>
        <div id="modalBody">
          <ModalCloseContext.Provider value={requestClose}>{children}</ModalCloseContext.Provider>
        </div>
      </section>
    </div>
  );
}
