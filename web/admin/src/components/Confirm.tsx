// window.confirm 대체 — 앱 디자인과 같은 확인 대화상자를 Promise로 사용한다.
import { createContext, useCallback, useContext, useRef, useState, type ReactNode } from 'react';
import { Modal, useModalClose } from './Modal';

interface ConfirmOptions {
  title: string;
  message?: ReactNode;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
}

type ConfirmFn = (opts: ConfirmOptions) => Promise<boolean>;

const ConfirmContext = createContext<ConfirmFn>(async () => false);

export const useConfirm = () => useContext(ConfirmContext);

function ConfirmBody({ opts, onAnswer }: { opts: ConfirmOptions; onAnswer: (v: boolean) => void }) {
  const close = useModalClose();
  const answer = (v: boolean) => { onAnswer(v); close(); };
  return (
    <>
      {opts.message && <div className="aq-confirm-message">{opts.message}</div>}
      <div className="aq-confirm-actions">
        {/* 지우기처럼 되돌릴 수 없는 확인은 Enter를 습관적으로 눌러도 실행되지 않게 '취소'에 먼저 포커스 */}
        <button type="button" className="ui-btn is-soft" onClick={() => answer(false)} {...(opts.danger ? { 'data-autofocus': true } : {})}>
          {opts.cancelLabel ?? '취소'}
        </button>
        <button
          type="button" {...(opts.danger ? {} : { 'data-autofocus': true })}
          className={`ui-btn ${opts.danger ? 'is-danger' : 'is-primary'}`}
          onClick={() => answer(true)}
        >
          {opts.confirmLabel ?? '확인'}
        </button>
      </div>
    </>
  );
}

export function ConfirmProvider({ children }: { children: ReactNode }) {
  const [opts, setOpts] = useState<ConfirmOptions | null>(null);
  const resolver = useRef<((v: boolean) => void) | null>(null);

  const confirm = useCallback<ConfirmFn>(o => {
    resolver.current?.(false);
    setOpts(o);
    return new Promise<boolean>(resolve => { resolver.current = resolve; });
  }, []);

  const onAnswer = (v: boolean) => {
    resolver.current?.(v);
    resolver.current = null;
  };

  return (
    <ConfirmContext.Provider value={confirm}>
      {children}
      {opts && (
        <Modal
          title={opts.title}
          modalClass="aq-confirm-mode"
          onClose={() => { onAnswer(false); setOpts(null); }}
        >
          <ConfirmBody opts={opts} onAnswer={onAnswer} />
        </Modal>
      )}
    </ConfirmContext.Provider>
  );
}
