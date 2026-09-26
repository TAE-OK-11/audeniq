import { useEffect, type ReactNode } from 'react';

interface ModalProps {
  title: string;
  onClose: () => void;
  children: ReactNode;
  /** #modal에 추가할 클래스 (예: aq-payout-setup-mode, aq-signature-mode) */
  modalClass?: string;
}

export function Modal({ title, onClose, children, modalClass }: ModalProps) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', onKey);
    document.body.style.overflow = 'hidden';
    return () => {
      document.removeEventListener('keydown', onKey);
      document.body.style.overflow = '';
    };
  }, [onClose]);

  return (
    <div
      id="modal"
      className={modalClass ? `modal ${modalClass}` : 'modal'}
      onClick={e => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <section className="modal-inner" role="dialog" aria-modal="true" aria-labelledby="modalTitle">
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
