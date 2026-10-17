// 선택 시트 — 앱 첫 화면에 늘 필요한 부분(<SelectHost/>와 공통 바탕 Surface)만 따로 둔다.
// 달력·시간·달 선택(Picker.tsx)은 쓰는 화면의 청크로 함께 빠져 첫 번들이 가벼워진다.
//
// <SelectHost/>를 앱에 한 번 두면 화면의 모든 <select>가 이 시트로 열린다. 원래 <select>는 그대로 남아
// 값·onChange·폼·테스트(selectOption)가 그대로 동작하고, 고르면 값만 바꾼 뒤 change 이벤트를 보낸다.
// 기본 팝업이 꼭 필요한 곳은 조상에 data-native-select를 붙인다.
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { coverOutsideFixedLayer } from './Modal';
import { Glyph } from './Glyph';

const SHEET_QUERY = '(max-width: 600px), (pointer: coarse) and (max-width: 900px)';
const isSheet = () => typeof window !== 'undefined' && !!window.matchMedia?.(SHEET_QUERY).matches;

// ---------------------------------------------------------------------------
// 공통 바탕: 휴대폰은 바텀 시트, 넓은 화면은 칸에 붙은 작은 창
// ---------------------------------------------------------------------------
export function Surface({ anchor, title, onClose, className, children, labelledBy, pop }: {
  anchor: DOMRect | null; title: string; onClose: () => void; className?: string; children: ReactNode; labelledBy?: string;
  /** 휴대폰에서도 시트 대신 칸 아래 작은 창으로 */
  pop?: boolean;
}) {
  const sheet = !pop && isSheet();
  const boxRef = useRef<HTMLDivElement>(null);
  const rootRef = useRef<HTMLDivElement>(null);

  // 닫힐 때 툭 사라지지 않게 — 사라지기 직전 모습을 그대로 복사해 두고 그 복사본이 내려가며(작은 창은 줄어들며) 사라진다.
  // 고르는 곳마다 닫기를 기다리게 바꾸지 않아도 되고, 원래 창은 바로 닫혀 값·포커스는 지금처럼 바로 돌아간다.
  useLayoutEffect(() => () => {
    const node = rootRef.current;
    if (!node || window.matchMedia?.('(prefers-reduced-motion: reduce)').matches) return;
    const ghost = node.cloneNode(true) as HTMLElement;
    ghost.classList.add('is-leaving');
    ghost.setAttribute('aria-hidden', 'true');
    ghost.inert = true;
    ghost.querySelectorAll('[id]').forEach(x => x.removeAttribute('id'));
    const scrollTop = boxRef.current?.scrollTop ?? 0;
    // 정말 닫힐 때만 — 개발 모드(StrictMode)가 열자마자 한 번 닫았다 다시 여는 경우엔 원래 창이 그대로 남아 있다
    queueMicrotask(() => {
      if (node.isConnected) return;
      document.body.appendChild(ghost);
      const box = ghost.querySelector<HTMLElement>('.aq-picker-box');
      if (box) box.scrollTop = scrollTop;
      // 내려가는(줄어드는) 애니메이션이 끝난 프레임에 치운다
      const end = (e: AnimationEvent) => { if (e.target === box) ghost.remove(); };
      box?.addEventListener('animationend', end);
      window.setTimeout(() => ghost.remove(), 600);
    });
  }, []);
  const [pos, setPos] = useState<{ left: number; top: number; width: number; maxHeight: number } | null>(null);

  useLayoutEffect(() => {
    if (sheet || !anchor) return;
    const box = boxRef.current;
    const vw = window.innerWidth, vh = window.innerHeight;
    const want = className?.includes('is-datetime') ? 540 : className?.includes('is-calendar') || className?.includes('is-time') ? 320 : 220;
    const width = Math.min(Math.max(anchor.width, want), vw - 24);
    const left = Math.min(Math.max(12, anchor.left), vw - width - 12);
    // 실제 너비를 먼저 준 뒤 높이를 재야 줄바꿈까지 맞는 높이가 나온다
    if (box) { box.style.width = `${width}px`; box.style.maxHeight = 'none'; }
    const natural = box ? box.scrollHeight : 320;
    const below = vh - anchor.bottom - 16, above = anchor.top - 16;
    // 다 들어가는 쪽(아래 우선), 둘 다 모자라면 더 넓은 쪽에 스크롤로
    const placeBelow = below >= natural || (above < natural && below >= above);
    const maxHeight = Math.max(160, Math.min(natural, placeBelow ? below : above));
    const top = placeBelow ? anchor.bottom + 6 : anchor.top - 6 - Math.min(natural, maxHeight);
    setPos({ left, top, width, maxHeight });
  }, [anchor, sheet, className]);

  // 휴대폰 선택 시트: 아래 주소창 뒤·화면 위 바깥 띠를 페이지 안 덮개로 메운다 (iOS 26 이후, Modal과 같은 방식)
  useEffect(() => {
    if (!sheet || !rootRef.current || !boxRef.current) return;
    return coverOutsideFixedLayer(rootRef.current, boxRef.current);
  }, [sheet]);

  // 휴대폰 시트가 열려 있는 동안 뒤 화면이 끌려 스크롤되지 않게
  useEffect(() => {
    if (!sheet) return;
    // html만 잠근다: body까지 잠그면 스크롤해 둔 화면에서 고정 머리글이 위로 밀려 사라진다
    const html = document.documentElement;
    const prev = html.style.overflow;
    html.style.overflow = 'hidden';
    return () => { html.style.overflow = prev; };
  }, [sheet]);

  // 넓은 화면: 바깥 스크롤·창 크기 변경이면 닫는다(칸에서 떨어져 떠 있지 않도록)
  useEffect(() => {
    if (sheet) return;
    const onScroll = (e: Event) => { if (!boxRef.current?.contains(e.target as Node)) onClose(); };
    window.addEventListener('scroll', onScroll, true);
    window.addEventListener('resize', onClose);
    return () => { window.removeEventListener('scroll', onScroll, true); window.removeEventListener('resize', onClose); };
  }, [sheet, onClose]);

  return createPortal(
    <div ref={rootRef} className={`aq-picker${sheet ? ' is-sheet' : ' is-pop'}`} onMouseDown={e => { if (e.target === e.currentTarget) onClose(); }}>
      <div
        ref={boxRef}
        className={`aq-picker-box${className ? ` ${className}` : ''}`}
        role="dialog" aria-modal={sheet || undefined} aria-label={labelledBy ? undefined : title || '선택'} aria-labelledby={labelledBy}
        style={sheet || !pos ? (sheet ? undefined : { visibility: 'hidden' }) : { left: pos.left, top: pos.top, width: pos.width, maxHeight: pos.maxHeight }}
      >
        {sheet && <span className="aq-picker-grip" aria-hidden="true" />}
        {sheet && title && <p className="aq-picker-title">{title}</p>}
        {children}
      </div>
    </div>,
    document.body,
  );
}

/** 열려 있는 동안 Esc·Tab을 이 창이 먼저 받는다(뒤 모달이 같이 닫히지 않게) */
export function useTopKeys(onKey: (e: KeyboardEvent) => void) {
  const ref = useRef(onKey);
  ref.current = onKey;
  useEffect(() => {
    const h = (e: KeyboardEvent) => ref.current(e);
    window.addEventListener('keydown', h, true);
    return () => window.removeEventListener('keydown', h, true);
  }, []);
}

// ---------------------------------------------------------------------------
// 선택 시트
// ---------------------------------------------------------------------------
type Opt = { value: string; label: string; disabled: boolean; group?: string };

function readOptions(el: HTMLSelectElement): Opt[] {
  const out: Opt[] = [];
  for (const child of Array.from(el.children)) {
    if (child instanceof HTMLOptGroupElement) {
      for (const o of Array.from(child.querySelectorAll('option'))) {
        if (!o.hidden) out.push({ value: o.value, label: o.label || o.text, disabled: o.disabled || child.disabled, group: child.label });
      }
    } else if (child instanceof HTMLOptionElement && !child.hidden) {
      // '선택해 주세요' 같은 안내용 빈 칸(고를 수 없음)은 목록에서 뺀다
      if (child.value === '' && child.disabled) continue;
      out.push({ value: child.value, label: child.label || child.text, disabled: child.disabled });
    }
  }
  return out;
}

export function fieldTitle(el: HTMLElement): string {
  const label = (el as HTMLSelectElement | HTMLInputElement).labels?.[0]?.textContent
    ?? el.getAttribute('aria-label')
    ?? (el.getAttribute('aria-labelledby') ? document.getElementById(el.getAttribute('aria-labelledby')!)?.textContent : '')
    ?? '';
  return label.replace(/\*/g, '').replace(/\s+/g, ' ').trim();
}

const eligible = (t: EventTarget | null): HTMLSelectElement | null => {
  if (!(t instanceof HTMLSelectElement)) return null;
  if (t.multiple || t.size > 1 || t.disabled || t.closest('[data-native-select]')) return null;
  return t;
};

function pickValue(el: HTMLSelectElement, value: string) {
  if (el.value === value) return;
  const setter = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value')?.set;
  if (setter) setter.call(el, value); else el.value = value;
  el.dispatchEvent(new Event('input', { bubbles: true }));
  el.dispatchEvent(new Event('change', { bubbles: true }));
}

function SelectSheet({ el, viaTouch, onClose }: { el: HTMLSelectElement; viaTouch: boolean; onClose: () => void }) {
  const [anchor] = useState(() => el.getBoundingClientRect());
  const [opts] = useState(() => readOptions(el));
  const current = el.value;
  const listRef = useRef<HTMLDivElement>(null);

  const close = useCallback((refocus = true) => {
    onClose();
    // 손가락으로 연 경우 select에 포커스를 돌리면 iOS가 기본 휠을 다시 띄우므로 키보드·마우스일 때만
    if (refocus && !viaTouch) el.focus({ preventScroll: true });
  }, [el, onClose, viaTouch]);

  useEffect(() => {
    const list = listRef.current;
    const sel = list?.querySelector<HTMLElement>('[aria-selected="true"]') ?? list?.querySelector<HTMLElement>('[role=option]:not([disabled])');
    // 손가락으로 열었을 때는 포커스 테두리 없이 고른 항목만 보이게 스크롤
    if (!viaTouch) sel?.focus({ preventScroll: true });
    sel?.scrollIntoView({ block: 'center' });
  }, [viaTouch]);

  useTopKeys(e => {
    if (e.key === 'Escape' || e.key === 'Tab') { e.preventDefault(); e.stopPropagation(); close(); return; }
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp' || e.key === 'Home' || e.key === 'End') {
      const items = Array.from(listRef.current?.querySelectorAll<HTMLButtonElement>('[role=option]:not([disabled])') ?? []);
      if (!items.length) return;
      e.preventDefault(); e.stopPropagation();
      const i = items.indexOf(document.activeElement as HTMLButtonElement);
      const next = e.key === 'Home' ? 0 : e.key === 'End' ? items.length - 1
        : e.key === 'ArrowDown' ? Math.min(items.length - 1, i + 1) : Math.max(0, i - 1);
      items[next].focus();
    }
  });

  const title = fieldTitle(el);
  let lastGroup: string | undefined;
  return (
    <Surface anchor={anchor} title={title} onClose={() => close()} className="is-select" pop>
      <div className="aq-picker-list" role="listbox" aria-label={title || '선택'} ref={listRef}>
        {opts.length === 0 && <p className="aq-picker-empty">고를 수 있는 항목이 없어요.</p>}
        {opts.map((o, i) => {
          const head = o.group && o.group !== lastGroup ? o.group : null;
          lastGroup = o.group;
          const on = o.value === current;
          return (
            <div key={`${o.value}-${i}`} role="presentation">
              {head && <p className="aq-picker-group">{head}</p>}
              <button
                type="button" role="option" aria-selected={on} disabled={o.disabled}
                className={`aq-picker-opt${on ? ' is-on' : ''}`}
                onClick={() => { pickValue(el, o.value); close(); }}
              >
                <span>{o.label}</span>
                {on && <Glyph name="check" size={18} />}
              </button>
            </div>
          );
        })}
      </div>
    </Surface>
  );
}

/** 앱에 한 번 두면 모든 <select>가 AUDENIQ 선택 시트로 열린다 */
export function SelectHost() {
  const [open, setOpen] = useState<{ el: HTMLSelectElement; viaTouch: boolean; n: number } | null>(null);

  useEffect(() => {
    let n = 0;
    let start: { x: number; y: number; el: HTMLSelectElement } | null = null;
    const show = (el: HTMLSelectElement, viaTouch: boolean) => setOpen({ el, viaTouch, n: ++n });

    const onMouseDown = (e: MouseEvent) => {
      const el = eligible(e.target);
      if (!el || e.button !== 0) return;
      e.preventDefault();
      el.focus({ preventScroll: true });
      show(el, false);
    };
    const onTouchStart = (e: TouchEvent) => {
      const el = eligible(e.target);
      const t = e.touches[0];
      start = el && t ? { x: t.clientX, y: t.clientY, el } : null;
    };
    const onTouchEnd = (e: TouchEvent) => {
      const el = eligible(e.target);
      const t = e.changedTouches[0];
      if (!el || !start || start.el !== el || !t) return;
      const moved = Math.hypot(t.clientX - start.x, t.clientY - start.y) > 10;
      start = null;
      if (moved) return;
      // 기본 동작(포커스 → iOS 휠·안드로이드 목록)을 막고 우리 시트를 연다
      e.preventDefault();
      if (document.activeElement instanceof HTMLElement && document.activeElement !== el) document.activeElement.blur();
      show(el, true);
    };
    const onKeyDown = (e: KeyboardEvent) => {
      const el = eligible(e.target);
      if (!el) return;
      if (e.key === ' ' || e.key === 'Enter' || ((e.key === 'ArrowDown' || e.key === 'ArrowUp') && (e.altKey || !e.shiftKey))) {
        e.preventDefault();
        show(el, false);
      }
    };
    document.addEventListener('mousedown', onMouseDown, true);
    document.addEventListener('touchstart', onTouchStart, { capture: true, passive: true });
    document.addEventListener('touchend', onTouchEnd, { capture: true, passive: false });
    document.addEventListener('keydown', onKeyDown, true);
    return () => {
      document.removeEventListener('mousedown', onMouseDown, true);
      document.removeEventListener('touchstart', onTouchStart, true);
      document.removeEventListener('touchend', onTouchEnd, true);
      document.removeEventListener('keydown', onKeyDown, true);
    };
  }, []);

  // 열려 있는 동안 칸에 표시를 달아 화살표가 위로 돌아가게 한다
  useEffect(() => {
    if (!open) return;
    const el = open.el;
    el.setAttribute('data-open', '');
    el.setAttribute('aria-expanded', 'true');
    return () => { el.removeAttribute('data-open'); el.removeAttribute('aria-expanded'); };
  }, [open]);

  // 시트가 떠 있는 동안 select가 화면에서 사라지면(페이지 이동 등) 닫는다
  useEffect(() => {
    if (!open) return;
    const t = window.setInterval(() => { if (!document.contains(open.el)) setOpen(null); }, 400);
    return () => window.clearInterval(t);
  }, [open]);

  const close = useCallback(() => setOpen(null), []);
  return open ? <SelectSheet key={open.n} el={open.el} viaTouch={open.viaTouch} onClose={close} /> : null;
}

// ---------------------------------------------------------------------------
// 달력
// ---------------------------------------------------------------------------
