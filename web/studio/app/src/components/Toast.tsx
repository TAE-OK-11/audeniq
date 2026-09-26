import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from 'react';

type ToastTone = 'info' | 'success' | 'error';
type ToastFn = (message: string, tone?: ToastTone) => void;

const ToastContext = createContext<ToastFn>(() => {});

export function useToast() {
  return useContext(ToastContext);
}

interface Item { id: number; message: string; tone: ToastTone; leaving: boolean }

const DURATION = 3200;
const MAX = 3;

/** 문구로 톤 추정 — 기존 호출부(toast('...'))를 바꾸지 않아도 성공/오류 색이 붙도록 */
function guessTone(msg: string): ToastTone {
  if (/실패|못했|없어요|확인해 주세요|입력해 주세요|선택해 주세요|할 수 없|초과|오류/.test(msg)) return 'error';
  if (/했어요|됐어요|완료|저장/.test(msg)) return 'success';
  return 'info';
}

export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<Item[]>([]);
  const seq = useRef(0);
  const timers = useRef(new Map<number, number>());

  const dismiss = useCallback((id: number) => {
    setItems(list => list.map(t => (t.id === id ? { ...t, leaving: true } : t)));
    window.setTimeout(() => setItems(list => list.filter(t => t.id !== id)), 220);
    const tm = timers.current.get(id);
    if (tm) { window.clearTimeout(tm); timers.current.delete(id); }
  }, []);

  const toast = useCallback<ToastFn>((message, tone) => {
    const id = ++seq.current;
    setItems(list => {
      // 같은 문구가 연달아 오면 새로 쌓지 않는다
      const filtered = list.filter(t => t.message !== message);
      return [...filtered, { id, message, tone: tone ?? guessTone(message), leaving: false }].slice(-MAX);
    });
    timers.current.set(id, window.setTimeout(() => dismiss(id), DURATION));
  }, [dismiss]);

  useEffect(() => {
    const map = timers.current;
    return () => { map.forEach(t => window.clearTimeout(t)); map.clear(); };
  }, []);

  return (
    <ToastContext.Provider value={toast}>
      {children}
      <div className="aq-toast-stack" role="status" aria-live="polite">
        {items.map(t => (
          <div key={t.id} className={`aq-toast is-${t.tone}${t.leaving ? ' is-leaving' : ''}`} onClick={() => dismiss(t.id)}>
            <span className="aq-toast-icon" aria-hidden="true">{t.tone === 'success' ? '✓' : t.tone === 'error' ? '!' : 'i'}</span>
            <span>{t.message}</span>
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  );
}
