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
        {/* 지우기처럼 되돌릴 수 없는 확인은 Enter를 습관적으로 눌러도 실행되지 않게 '닫기'에 먼저 포커스 */}
        <button type="button" className="ui-btn is-soft" onClick={() => answer(false)} {...(opts.danger ? { 'data-autofocus': true } : {})}>
          {opts.cancelLabel ?? '닫기'}
        </button>
        <button
          type="button" {...(opts.danger ? {} : { 'data-autofocus': true })}
          className={`ui-btn ${opts.danger ? 'is-danger-solid' : 'is-primary'}`}
          onClick={() => answer(true)}
        >
          {opts.confirmLabel ?? '확인'}
        </button>
      </div>
    </>
  );
}

export function ConfirmProvider({ children }: { children: ReactNode }) {
  // 요청마다 번호 — 앞 창이 닫히는 애니메이션(최대 0.6초) 중에 이어서 연 확인 창이, 앞 창이 다 닫히는 순간
  // 같이 '취소'로 닫혀 버렸다. 창을 번호로 새로 그리고, 닫힘은 자기 요청에만 답한다
  const [opts, setOpts] = useState<(ConfirmOptions & { seq: number }) | null>(null);
  const resolver = useRef<((v: boolean) => void) | null>(null);
  const seq = useRef(0);

  const confirm = useCallback<ConfirmFn>(o => {
    resolver.current?.(false);
    setOpts({ ...o, seq: ++seq.current });
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
          key={opts.seq}
          title={opts.title}
          modalClass="aq-confirm-mode"
          onClose={() => {
            if (opts.seq !== seq.current) return;
            onAnswer(false);
            setOpts(cur => (cur?.seq === opts.seq ? null : cur));
          }}
        >
          <ConfirmBody opts={opts} onAnswer={onAnswer} />
        </Modal>
      )}
    </ConfirmContext.Provider>
  );
}
