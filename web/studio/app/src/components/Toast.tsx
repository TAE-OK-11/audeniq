// 토스트 — 토스 TDS 뼈대(화면 폭 둥근 사각형 · 24px 아이콘 · 글자 버튼 하나 · 고정 버튼 위)에 AUDENIQ 네이비·움직임.
// toast('저장했어요') 처럼 문구만 넘기면 예전처럼 동작하고, 두 번째 인자로 옵션을 넘기면
// 둘째 줄·버튼(되돌리기·다시 시도)·진행 중 상태를 쓸 수 있다. 돌려받은 handle로 같은 토스트를 제자리에서 바꾼다
// (예: '올리는 중' → '올렸어요'). 같은 문구가 또 오면 쌓지 않고 ×2 배지를 붙인다.
import { createContext, useCallback, useContext, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { STORAGE_FAIL_EVENT } from '../lib/storage';

export type ToastTone = 'info' | 'success' | 'error' | 'loading';
export interface ToastOptions {
  tone?: ToastTone;
  /** 둘째 줄 — 이유나 다음 할 일 */
  sub?: string;
  /** 버튼 하나 — '되돌리기'·'다시 시도'·'보기'처럼 실행만 ('닫기'·'확인'은 쓰지 않는다) */
  action?: { label: string; onClick: (toast: ToastHandle) => void };
  /** 보이는 시간(ms). 진행 중(loading)은 기본으로 끝날 때까지 */
  duration?: number;
}
export interface ToastHandle {
  /** 같은 자리에서 문구·상태를 바꾼다. 시간은 새로 센다 */
  update: (message: string, opts?: ToastTone | ToastOptions) => void;
  close: () => void;
}
type ToastFn = (message: string, opts?: ToastTone | ToastOptions) => ToastHandle;

const NOOP: ToastHandle = { update: () => {}, close: () => {} };
const ToastContext = createContext<ToastFn>(() => NOOP);

export function useToast() {
  return useContext(ToastContext);
}

interface Item {
  id: number; message: string; tone: ToastTone; sub?: string; action?: ToastOptions['action']; duration?: number;
  count: number; rev: number; leaving: boolean; fling: boolean;
}

/** 오류·버튼이 있으면 읽고 누를 시간이 더 필요하다. 긴 문구는 글자 수만큼 조금 더 둔다 */
const lifeFor = (t: Item) => t.duration ?? (t.tone === 'loading' ? Infinity
  : Math.min(9000, (t.tone === 'error' || t.action ? 5000 : 3000) + Math.max(0, (t.message + (t.sub ?? '')).length - 30) * 45));
/** 보이는 토스트 수 — 셋째부터는 뒤로 밀려 사라진다 */
const MAX = 2;
const LEAVE_MS = 270;
const FLING_MS = 220;

/** 문구로 톤 추정 — 기존 호출부(toast('...'))를 바꾸지 않아도 성공/오류 색이 붙도록 */
function guessTone(msg: string): ToastTone {
  if (/실패|못했|없어요|확인해 주세요|입력해 주세요|선택해 주세요|할 수 없|초과|오류/.test(msg)) return 'error';
  if (/했어요|됐어요|올렸어요|보냈어요|받았어요|마쳤어요|완료|저장/.test(msg)) return 'success';
  return 'info';
}
const normalize = (message: string, opts?: ToastTone | ToastOptions): ToastOptions & { tone: ToastTone } => {
  const o = typeof opts === 'string' ? { tone: opts } : (opts ?? {});
  return { ...o, tone: o.tone ?? guessTone(message) };
};

export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<Item[]>([]);
  const seq = useRef(0);
  const stackRef = useRef<HTMLDivElement>(null);
  const els = useRef(new Map<number, HTMLDivElement>());
  const [kb, setKb] = useState(0);
  const [hidden, setHidden] = useState(() => typeof document !== 'undefined' && document.hidden);

  const dismiss = useCallback((id: number, fling = false) => {
    setItems(list => list.map(t => (t.id === id && !t.leaving ? { ...t, leaving: true, fling } : t)));
    window.setTimeout(() => setItems(list => list.filter(t => t.id !== id)), fling ? FLING_MS : LEAVE_MS);
  }, []);

  const handleFor = useCallback((id: number): ToastHandle => ({
    update: (message, opts) => {
      const o = normalize(message, opts);
      setItems(list => list.map(t => (t.id === id && !t.leaving
        ? { ...t, message, tone: o.tone, sub: o.sub, action: o.action, duration: o.duration, count: 1, rev: t.rev + 1 } : t)));
    },
    close: () => dismiss(id),
  }), [dismiss]);

  const itemsRef = useRef(items);
  itemsRef.current = items;
  const toast = useCallback<ToastFn>((message, opts) => {
    const o = normalize(message, opts);
    // 같은 문구가 연달아 오면 새로 쌓지 않고 ×2 — 버튼이 있는 토스트는 각자 따로 둔다
    const same = !o.action && itemsRef.current.find(t => !t.leaving && !t.action && t.message === message && t.tone === o.tone && t.sub === o.sub);
    if (same) {
      setItems(list => list.map(t => (t.id === same.id ? { ...t, count: t.count + 1, rev: t.rev + 1 } : t)));
      return handleFor(same.id);
    }
    const id = ++seq.current;
    setItems(list => [...list, { id, message, tone: o.tone, sub: o.sub, action: o.action, duration: o.duration, count: 1, rev: 0, leaving: false, fling: false }]);
    // 보이는 수를 넘으면 가장 오래된 것부터 닫는다
    const live = itemsRef.current.filter(t => !t.leaving);
    if (live.length >= MAX) dismiss(live[0].id);
    return handleFor(id);
  }, [dismiss, handleFor]);

  // 하나가 사라지거나 새로 오면 남은 토스트가 이전 자리에서 새 자리로 미끄러진다 (FLIP).
  // 아래에 붙은 쌓임이라 '아래에서 얼마나 떨어졌나'로 잰다
  const fromBottom = useRef(new Map<number, number>());
  useLayoutEffect(() => {
    const stack = stackRef.current;
    if (!stack) return;
    const h = stack.offsetHeight;
    const next = new Map<number, number>();
    els.current.forEach((el, id) => {
      const d = h - el.offsetTop;
      next.set(id, d);
      const before = fromBottom.current.get(id);
      if (before == null || before === d || el.classList.contains('is-leaving')) return;
      el.style.transition = 'none';
      el.style.translate = `0 ${d - before}px`;
      void el.offsetWidth;
      el.style.transition = 'translate .42s var(--aq-enter), scale .4s var(--aq-enter), background-color .35s ease';
      el.style.translate = '';
      window.setTimeout(() => { el.style.transition = ''; }, 450);
    });
    fromBottom.current = next;
  }, [items]);

  // 화면 키보드가 열리면 키보드 위로 따라 올라간다
  useEffect(() => {
    const vv = window.visualViewport;
    if (!vv) return;
    const on = () => {
      const covered = window.innerHeight - vv.height - vv.offsetTop;
      setKb(covered > 80 ? covered : 0);
    };
    vv.addEventListener('resize', on);
    vv.addEventListener('scroll', on);
    return () => { vv.removeEventListener('resize', on); vv.removeEventListener('scroll', on); };
  }, []);

  // 다른 탭을 보는 동안은 시간을 멈춘다
  useEffect(() => {
    const on = () => setHidden(document.hidden);
    document.addEventListener('visibilitychange', on);
    return () => document.removeEventListener('visibilitychange', on);
  }, []);

  // Esc로 가장 새 토스트를 닫는다 — 창(모달)이 열려 있으면 Esc는 창 몫이다
  useEffect(() => {
    const on = (e: KeyboardEvent) => {
      if (e.key !== 'Escape' || e.defaultPrevented || document.querySelector('#modal.is-open, [role="dialog"], [role="alertdialog"]')) return;
      const last = [...itemsRef.current].reverse().find(t => !t.leaving);
      if (last) dismiss(last.id);
    };
    document.addEventListener('keydown', on);
    return () => document.removeEventListener('keydown', on);
  }, [dismiss]);

  // 브라우저 저장 공간 부족 등으로 저장이 실패하면 알림
  useEffect(() => {
    let last = 0;
    const onFail = (e: Event) => {
      const now = Date.now();
      if (now - last < 5000) return; // 연속 실패 시 알림 폭주 방지
      last = now;
      const quota = (e as CustomEvent<{ quota: boolean }>).detail?.quota;
      toast(quota ? '브라우저 저장 공간이 부족해 변경 내용을 저장하지 못했어요' : '변경 내용을 이 브라우저에 저장하지 못했어요', {
        tone: 'error',
        sub: quota ? '사용하지 않는 발매나 커버 이미지를 정리해 주세요.' : '개인정보 보호 모드인지 확인해 주세요.',
      });
    };
    window.addEventListener(STORAGE_FAIL_EVENT, onFail);
    return () => window.removeEventListener(STORAGE_FAIL_EVENT, onFail);
  }, [toast]);

  const live = items.filter(t => !t.leaving);
  return (
    <ToastContext.Provider value={toast}>
      {children}
      <div ref={stackRef} className={`aq-toast-stack${hidden ? ' is-paused' : ''}`} aria-live="polite"
        style={kb ? { bottom: `${kb + 12}px` } : undefined}>
        {items.map(t => {
          const back = t.leaving ? 0 : live.length - 1 - live.indexOf(t);
          return (
            <ToastItem
              key={t.id} item={t} back={back} paused={hidden}
              register={el => { if (el) els.current.set(t.id, el); else els.current.delete(t.id); }}
              onClose={fling => dismiss(t.id, fling)} handle={handleFor(t.id)}
            />
          );
        })}
      </div>
    </ToastContext.Provider>
  );
}

function Glyph({ tone }: { tone: ToastTone }) {
  if (tone === 'loading') {
    return <svg className="aq-toast-spin" viewBox="0 0 24 24"><circle className="track" cx="12" cy="12" r="9.5" /><circle className="arc" cx="12" cy="12" r="9.5" /></svg>;
  }
  return (
    <svg className="aq-toast-glyph" width="13" height="13" viewBox="0 0 16 16">
      {tone === 'success' && <path className="aq-toast-draw" d="M3.4 8.5 6.6 11.6 12.7 4.8" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round" />}
      {tone === 'error' && <><path d="M8 3.6v5.2" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" /><circle cx="8" cy="12.2" r="1.45" fill="currentColor" /></>}
      {tone === 'info' && <><circle cx="8" cy="3.9" r="1.45" fill="currentColor" /><path d="M8 7.2v5.2" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" /></>}
    </svg>
  );
}

function ToastItem({ item: t, back, paused, register, onClose, handle }: {
  item: Item; back: number; paused: boolean;
  register: (el: HTMLDivElement | null) => void;
  onClose: (fling?: boolean) => void;
  handle: ToastHandle;
}) {
  const [fresh, setFresh] = useState(true);
  const [pressed, setPressed] = useState(false);
  const [held, setHeld] = useState(false);
  const holds = useRef(new Set<string>());
  const timer = useRef(0);
  const left = useRef(Infinity);
  const started = useRef(0);
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  const life = lifeFor(t);

  const run = useCallback(() => {
    window.clearTimeout(timer.current);
    if (!Number.isFinite(left.current) || holds.current.size) return;
    started.current = performance.now();
    timer.current = window.setTimeout(() => closeRef.current(), left.current);
  }, []);
  // 멈췄던 만큼 그대로 이어서 센다 (다시 셀 때 시간을 줄이지 않는다)
  const hold = useCallback((why: string) => {
    if (!holds.current.size && Number.isFinite(left.current)) {
      window.clearTimeout(timer.current);
      left.current -= performance.now() - started.current;
    }
    holds.current.add(why);
    setHeld(true);
  }, []);
  const release = useCallback((why: string) => {
    if (!holds.current.delete(why) || holds.current.size) return;
    setHeld(false);
    left.current = Math.max(left.current, 1200);
    run();
  }, [run]);

  // 처음 뜰 때, 내용이 바뀌거나 같은 문구가 또 올 때 시간을 새로 센다
  useEffect(() => {
    left.current = life;
    run();
    return () => window.clearTimeout(timer.current);
  }, [t.rev, life, run]);
  useEffect(() => { if (paused) hold('tab'); else release('tab'); }, [paused, hold, release]);
  useEffect(() => { const id = window.setTimeout(() => setFresh(false), 800); return () => window.clearTimeout(id); }, []);

  // 내용이 바뀌면 새 글자가 아래에서 올라온다
  const bodyRef = useRef<HTMLSpanElement>(null);
  const firstRev = useRef(t.rev);
  useLayoutEffect(() => {
    const b = bodyRef.current;
    if (!b || t.rev === firstRev.current) return;
    b.classList.remove('is-swap');
    void b.offsetWidth;
    b.classList.add('is-swap');
  }, [t.rev, t.message]);

  // 옆·아래로 밀어 닫기 — 빠르게 튕기면 짧게 밀어도 닫히고, 위로는 고무줄처럼 버틴다
  const drag = useRef<{ x: number; y: number; pts: [number, number, number][]; moved: boolean } | null>(null);
  const swiped = useRef(false);
  const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if ((e.target as HTMLElement).closest('.aq-toast-act')) return;
    drag.current = { x: e.clientX, y: e.clientY, pts: [[e.clientX, e.clientY, e.timeStamp]], moved: false };
    e.currentTarget.setPointerCapture?.(e.pointerId);
    e.currentTarget.style.transition = '';
    setPressed(true);
    hold('press');
  };
  const onPointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (!d) return;
    let dx = e.clientX - d.x, dy = e.clientY - d.y;
    if (Math.hypot(dx, dy) > 6) d.moved = true;
    if (dy < 0) dy = -Math.sqrt(-dy) * 2.2;
    if (Math.abs(dx) > Math.abs(dy)) dy = 0; else dx = 0;
    const el = e.currentTarget;
    el.style.translate = `${dx}px ${dy}px`;
    el.style.opacity = String(Math.max(0.25, 1 - Math.max(Math.abs(dx), dy) / 220));
    d.pts.push([e.clientX, e.clientY, e.timeStamp]);
    if (d.pts.length > 6) d.pts.shift();
  };
  const onPointerEnd = (e: React.PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (!d) return;
    drag.current = null;
    const el = e.currentTarget;
    setPressed(false);
    const dx = e.clientX - d.x, dy = e.clientY - d.y;
    const a = d.pts[0], b = d.pts[d.pts.length - 1];
    const dt = Math.max(1, b[2] - a[2]);
    const vx = (b[0] - a[0]) / dt, vy = (b[1] - a[1]) / dt;
    const horiz = Math.abs(dx) > Math.abs(dy);
    swiped.current = d.moved;
    if (e.type === 'pointerup' && ((horiz && (Math.abs(dx) > 70 || (Math.abs(dx) > 24 && Math.abs(vx) > 0.45))) || (!horiz && (dy > 40 || (dy > 16 && vy > 0.45))))) {
      el.style.transition = 'translate .22s var(--aq-exit), opacity .22s ease';
      el.style.translate = horiz ? `${Math.sign(dx || vx) * 110}% 0` : `0 ${Math.max(dy, 0) + 80}px`;
      el.style.opacity = '0';
      onClose(true);
      return;
    }
    el.style.transition = 'translate .35s var(--toast-spring), opacity .2s ease';
    el.style.translate = '';
    el.style.opacity = '';
    window.setTimeout(() => { if (!drag.current) el.style.transition = ''; }, 360);
    release('press');
  };

  const cls = ['aq-toast', `is-${t.tone}`];
  if (t.sub) cls.push('has-sub');
  if (back === 1) cls.push('is-back');
  if (back >= 2) cls.push('is-back2');
  if (fresh) cls.push('is-fresh');
  if (held) cls.push('is-held');
  if (pressed) cls.push('is-pressed');
  if (t.leaving) cls.push(t.fling ? 'is-fling' : 'is-leaving');
  else if (fresh) cls.push('is-in');
  const ring = !!t.action && Number.isFinite(life);

  return (
    <div
      ref={register}
      className={cls.join(' ')}
      role={t.tone === 'error' ? 'alert' : 'status'}
      onClick={() => { if (swiped.current) { swiped.current = false; return; } onClose(); }}
      onPointerEnter={() => hold('hover')}
      onPointerLeave={() => { if (!drag.current) release('hover'); }}
      onFocus={() => hold('focus')}
      onBlur={() => release('focus')}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerEnd}
      onPointerCancel={onPointerEnd}
      style={ring ? ({ '--life': `${life}ms` } as React.CSSProperties) : undefined}
    >
      <span className="aq-toast-icon" aria-hidden="true">
        <Glyph key={`${t.tone}-${t.rev}`} tone={t.tone} />
        {ring && <svg key={`ring-${t.rev}`} className="aq-toast-ring" viewBox="0 0 32 32"><circle cx="16" cy="16" r="14" /></svg>}
      </span>
      <span ref={bodyRef} className="aq-toast-body">
        <span className="aq-toast-line">
          {t.message}
          {t.count > 1 && <span key={t.count} className="aq-toast-count" aria-label={`${t.count}번`}>×{t.count}</span>}
        </span>
        {t.sub && <span className="aq-toast-sub">{t.sub}</span>}
      </span>
      {t.action && (
        <button type="button" className="aq-toast-act" onClick={e => { e.stopPropagation(); t.action!.onClick(handle); }}>
          {t.action.label}
        </button>
      )}
    </div>
  );
}
