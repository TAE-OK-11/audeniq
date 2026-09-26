import { useEffect, useRef, type ReactNode } from 'react';

interface ModalProps {
  title: string;
  onClose: () => void;
  children: ReactNode;
  /** #modal에 추가할 클래스 (예: aq-payout-setup-mode, aq-signature-mode) */
  modalClass?: string;
}

// body 스크롤 잠금 카운터 — 중첩 모달에서 하나가 닫혀도 잠금 유지
let lockCount = 0;
function lockScroll() {
  lockCount += 1;
  if (lockCount === 1) document.body.style.overflow = 'hidden';
}
function unlockScroll() {
  lockCount = Math.max(0, lockCount - 1);
  if (lockCount === 0) document.body.style.overflow = '';
}

export function Modal({ title, onClose, children, modalClass }: ModalProps) {
  const innerRef = useRef<HTMLElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        onCloseRef.current();
        return;
      }
      // 포커스 트랩: Tab이 모달 안에서만 순환
      if (e.key === 'Tab') {
        const root = innerRef.current;
        if (!root) return;
        const focusables = root.querySelectorAll<HTMLElement>(
          'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
        );
        const list = Array.from(focusables).filter(el => !el.hasAttribute('disabled') && el.offsetParent !== null);
        if (list.length === 0) return;
        const first = list[0];
        const last = list[list.length - 1];
        if (e.shiftKey && document.activeElement === first) {
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
    // 열릴 때 모달 안 첫 포커스 가능 요소로 포커스 이동, 닫힐 때 복원
    const prevFocus = document.activeElement as HTMLElement | null;
    const t = window.setTimeout(() => {
      const root = innerRef.current;
      const target = root?.querySelector<HTMLElement>(
        'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
      );
      (target ?? root)?.focus();
    }, 0);
    return () => {
      window.clearTimeout(t);
      document.removeEventListener('keydown', onKey);
      unlockScroll();
      prevFocus?.focus?.();
    };
  }, []);

  return (
    <div
      id="modal"
      className={modalClass ? `modal ${modalClass}` : 'modal'}
      onClick={e => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <section
        className="modal-inner" ref={innerRef} tabIndex={-1}
        role="dialog" aria-modal="true" aria-labelledby="modalTitle"
      >
        <div className="modal-top">
          <h2 id="modalTitle">{title}</h2>
          <button className="icon-button" type="button" aria-label="닫기" onClick={onClose}>
            ×
          </button>
        </div>
        <div id="modalBody">{children}</div>
      </section>
    </div>
  );
}
