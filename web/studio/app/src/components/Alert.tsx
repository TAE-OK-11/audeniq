// 꼭 읽고 고쳐야 하는 안내 — 잠깐 떴다 사라지는 토스트 대신, 수령 계좌 등록의 ‘계좌번호를 다시 확인해 주세요’와 같은 대화상자.
// const alert = useAlert(); alert('제목', '자세한 안내');
import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';

type AlertFn = (title: string, detail?: string) => void;
const AlertContext = createContext<AlertFn>(() => {});
export const useAlert = () => useContext(AlertContext);

export function AlertProvider({ children }: { children: ReactNode }) {
  const [msg, setMsg] = useState<{ title: string; detail?: string } | null>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const backRef = useRef<Element | null>(null);
  const show = useCallback<AlertFn>((title, detail) => {
    backRef.current = document.activeElement;
    setMsg({ title, detail });
  }, []);
  const close = useCallback(() => {
    setMsg(null);
    (backRef.current as HTMLElement | null)?.focus?.();
  }, []);

  useEffect(() => {
    if (!msg) return;
    closeRef.current?.focus();
    // 위에 뜬 대화상자만 닫히도록 캡처 단계에서 Esc를 먼저 받는다
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' || e.key === 'Tab') { e.preventDefault(); e.stopPropagation(); if (e.key === 'Escape') close(); }
    };
    document.addEventListener('keydown', onKey, true);
    return () => document.removeEventListener('keydown', onKey, true);
  }, [msg, close]);

  return (
    <AlertContext.Provider value={show}>
      {children}
      {msg && createPortal(
        <div className="aq-pay-error-overlay aq-alert-overlay" role="alertdialog" aria-modal="true" aria-labelledby="aqAlertTitle" aria-describedby={msg.detail ? 'aqAlertDetail' : undefined}>
          <div className="aq-pay-error-dialog">
            <strong id="aqAlertTitle">{msg.title}</strong>
            {msg.detail ? <p id="aqAlertDetail">{msg.detail}</p> : <p aria-hidden="true" />}
            <button type="button" ref={closeRef} onClick={close}>확인</button>
          </div>
        </div>,
        document.body,
      )}
    </AlertContext.Provider>
  );
}
