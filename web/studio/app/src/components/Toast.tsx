import { Glyph } from './Glyph';
import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from 'react';
import { STORAGE_FAIL_EVENT } from '../lib/storage';
import { CheckIcon } from './Check';

type ToastTone = 'info' | 'success' | 'error';
type ToastFn = (message: string, tone?: ToastTone) => void;

const ToastContext = createContext<ToastFn>(() => {});

export function useToast() {
  return useContext(ToastContext);
}

interface Item { id: number; message: string; tone: ToastTone; leaving: boolean }

const DURATION = 3200;
/** 오류는 읽고 대처할 시간이 더 필요하다. 긴 문구는 글자 수만큼 조금 더 둔다 */
const durationFor = (message: string, tone: ToastTone) =>
  Math.min(9000, (tone === 'error' ? 5200 : DURATION) + Math.max(0, message.length - 30) * 45);
const MAX = 3;
/** 옆으로 이만큼 밀면 닫는다 */
const SWIPE_PX = 64;

/** 문구로 톤 추정 — 기존 호출부(toast('...'))를 바꾸지 않아도 성공/오류 색이 붙도록 */
function guessTone(msg: string): ToastTone {
  if (/실패|못했|없어요|확인해 주세요|입력해 주세요|선택해 주세요|할 수 없|초과|오류/.test(msg)) return 'error';
  if (/했어요|됐어요|올렸어요|보냈어요|받았어요|마쳤어요|완료|저장/.test(msg)) return 'success';
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
    const t = tone ?? guessTone(message);
    timers.current.set(id, window.setTimeout(() => dismiss(id), durationFor(message, t)));
  }, [dismiss]);

  // 누르고 있거나 마우스를 올려 두면 멈추고, 떼면 다시 센다 (읽는 중에 사라지지 않게)
  const hold = useCallback((id: number) => {
    const tm = timers.current.get(id);
    if (tm) { window.clearTimeout(tm); timers.current.delete(id); }
  }, []);
  const resume = useCallback((id: number, message: string, tone: ToastTone) => {
    if (timers.current.has(id)) return;
    timers.current.set(id, window.setTimeout(() => dismiss(id), Math.max(1800, durationFor(message, tone) / 2)));
  }, [dismiss]);
  const drag = useRef<{ id: number; x: number; el: HTMLElement } | null>(null);
  /** 방금 손가락으로 민 경우 뒤따르는 클릭(닫기)을 무시한다 */
  const swiped = useRef(false);

  // 브라우저 저장 공간 부족 등으로 저장이 실패하면 알림
  useEffect(() => {
    let last = 0;
    const onFail = (e: Event) => {
      const now = Date.now();
      if (now - last < 5000) return; // 연속 실패 시 알림 폭주 방지
      last = now;
      const quota = (e as CustomEvent<{ quota: boolean }>).detail?.quota;
      toast(quota
        ? '브라우저 저장 공간이 부족해 변경 내용을 저장하지 못했어요. 사용하지 않는 발매나 커버 이미지를 정리해 주세요.'
        : '변경 내용을 이 브라우저에 저장하지 못했어요. 개인정보 보호 모드인지 확인해 주세요.', 'error');
    };
    window.addEventListener(STORAGE_FAIL_EVENT, onFail);
    return () => window.removeEventListener(STORAGE_FAIL_EVENT, onFail);
  }, [toast]);

  useEffect(() => {
    const map = timers.current;
    return () => { map.forEach(t => window.clearTimeout(t)); map.clear(); };
  }, []);

  return (
    <ToastContext.Provider value={toast}>
      {children}
      <div className="aq-toast-stack" role="status" aria-live="polite">
        {items.map(t => (
          <div
            key={t.id} className={`aq-toast is-${t.tone}${t.leaving ? ' is-leaving' : ''}`}
            onClick={() => { if (swiped.current) { swiped.current = false; return; } dismiss(t.id); }}
            onPointerEnter={() => hold(t.id)}
            onPointerLeave={() => { if (!drag.current) resume(t.id, t.message, t.tone); }}
            onPointerDown={e => { hold(t.id); drag.current = { id: t.id, x: e.clientX, el: e.currentTarget }; e.currentTarget.setPointerCapture?.(e.pointerId); }}
            onPointerMove={e => {
              const d = drag.current;
              if (!d || d.id !== t.id) return;
              const dx = e.clientX - d.x;
              d.el.style.transition = 'none';
              d.el.style.transform = `translateX(${dx}px)`;
              d.el.style.opacity = String(Math.max(0.2, 1 - Math.abs(dx) / 200));
            }}
            onPointerUp={e => {
              const d = drag.current;
              if (!d || d.id !== t.id) return;
              const dx = e.clientX - d.x;
              d.el.style.transition = '';
              if (Math.abs(dx) > SWIPE_PX) {
                d.el.style.transform = `translateX(${dx > 0 ? 120 : -120}%)`;
                d.el.style.opacity = '0';
                window.setTimeout(() => setItems(list => list.filter(x => x.id !== t.id)), 180);
              } else {
                d.el.style.transform = '';
                d.el.style.opacity = '';
                resume(t.id, t.message, t.tone);
              }
              // 살짝이라도 민 것은 클릭(닫기)으로 치지 않는다
              swiped.current = Math.abs(dx) > 6;
              drag.current = null;
            }}
            onPointerCancel={() => { const d = drag.current; if (d) { d.el.style.transform = ''; d.el.style.opacity = ''; drag.current = null; resume(t.id, t.message, t.tone); } }}
          >
            <span className="aq-toast-icon" aria-hidden="true">{t.tone === 'success' ? <CheckIcon size={12} /> : <Glyph name={t.tone === 'error' ? 'alert' : 'info'} size={14} />}</span>
            <span>{t.message}</span>
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  );
}
