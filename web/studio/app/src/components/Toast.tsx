import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from 'react';

const ToastContext = createContext<(message: string) => void>(() => {});

export function useToast() {
  return useContext(ToastContext);
}

const DISPLAY_MS = 2800;

export function ToastProvider({ children }: { children: ReactNode }) {
  const [queue, setQueue] = useState<string[]>([]);
  const [current, setCurrent] = useState<string | null>(null);
  const timer = useRef<number | null>(null);

  const toast = useCallback((msg: string) => {
    setQueue(q => [...q, msg]);
  }, []);

  // 큐에서 하나씩 표시 — 연속 호출 시 앞 메시지가 유실되지 않음
  useEffect(() => {
    if (current || queue.length === 0) return;
    const [next, ...rest] = queue;
    setCurrent(next);
    setQueue(rest);
    timer.current = window.setTimeout(() => setCurrent(null), DISPLAY_MS);
    return () => {
      if (timer.current) window.clearTimeout(timer.current);
    };
  }, [current, queue]);

  return (
    <ToastContext.Provider value={toast}>
      {children}
      <div
        id="toast"
        className={`toast${current ? ' show' : ''}`}
        role="status" aria-live="polite"
      >
        {current}
      </div>
    </ToastContext.Provider>
  );
}
