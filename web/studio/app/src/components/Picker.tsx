// AUDENIQ 선택 시트 · 달력 — 기기마다 모양이 다른 기본 선택 팝업(iOS 휠, 안드로이드 다이얼로그, 데스크톱 목록)과
// 기본 달력 대신 어디서나 같은 모양으로 보여 준다. 선택 목록은 어느 기기든 칸 바로 아래 작은 창,
// 달력·시간은 휴대폰에서 아래에서 올라오는 시트(넓은 화면은 칸 아래 작은 창).
//
// <SelectHost/>를 앱에 한 번 두면 화면의 모든 <select>가 이 시트로 열린다. 원래 <select>는 그대로 남아
// 값·onChange·폼·테스트(selectOption)가 그대로 동작하고, 고르면 값만 바꾼 뒤 change 이벤트를 보낸다.
// 기본 팝업이 꼭 필요한 곳은 조상에 data-native-select를 붙인다.
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { Glyph } from './Glyph';

const SHEET_QUERY = '(max-width: 600px), (pointer: coarse) and (max-width: 900px)';
const isSheet = () => typeof window !== 'undefined' && !!window.matchMedia?.(SHEET_QUERY).matches;

// ---------------------------------------------------------------------------
// 공통 바탕: 휴대폰은 바텀 시트, 넓은 화면은 칸에 붙은 작은 창
// ---------------------------------------------------------------------------
function Surface({ anchor, title, onClose, className, children, labelledBy, pop }: {
  anchor: DOMRect | null; title: string; onClose: () => void; className?: string; children: ReactNode; labelledBy?: string;
  /** 휴대폰에서도 시트 대신 칸 아래 작은 창으로 */
  pop?: boolean;
}) {
  const sheet = !pop && isSheet();
  const boxRef = useRef<HTMLDivElement>(null);
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

  // 휴대폰 시트가 열려 있는 동안 뒤 화면이 끌려 스크롤되지 않게
  useEffect(() => {
    if (!sheet) return;
    const html = document.documentElement, body = document.body;
    const prev = [html.style.overflow, body.style.overflow];
    html.style.overflow = 'hidden'; body.style.overflow = 'hidden';
    return () => { html.style.overflow = prev[0]; body.style.overflow = prev[1]; };
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
    <div className={`aq-picker${sheet ? ' is-sheet' : ' is-pop'}`} onMouseDown={e => { if (e.target === e.currentTarget) onClose(); }}>
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
function useTopKeys(onKey: (e: KeyboardEvent) => void) {
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

function fieldTitle(el: HTMLElement): string {
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
const pad = (n: number) => String(n).padStart(2, '0');
const iso = (y: number, m: number, d: number) => `${y}-${pad(m + 1)}-${pad(d)}`;
const parseIso = (v?: string) => {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(v ?? '');
  return m ? { y: +m[1], m: +m[2] - 1, d: +m[3] } : null;
};
const todayIso = () => { const t = new Date(); return iso(t.getFullYear(), t.getMonth(), t.getDate()); };
export const formatDateKo = (v: string) => {
  const p = parseIso(v);
  return p ? `${p.y}년 ${p.m + 1}월 ${p.d}일` : '';
};
const WEEK = ['일', '월', '화', '수', '목', '금', '토'];

function Calendar({ value, min, max, required, onPick, onClear, onClose, title, autoFocus, foot = true }: {
  value: string; min?: string; max?: string; required?: boolean; title: string; autoFocus: boolean; foot?: boolean;
  onPick: (v: string) => void; onClear: () => void; onClose: () => void;
}) {
  const today = todayIso();
  const start = parseIso(value) ?? parseIso(min && min > today ? min : today)!;
  const [view, setView] = useState({ y: start.y, m: start.m });
  const [mode, setMode] = useState<'day' | 'year' | 'month'>('day');
  const [focus, setFocus] = useState(iso(start.y, start.m, start.d));
  const gridRef = useRef<HTMLDivElement>(null);
  const yearsRef = useRef<HTMLDivElement>(null);
  // 키보드로 열었을 때만 날짜 칸에 포커스(손가락·마우스로 열면 오늘 칸이 고른 것처럼 보이지 않게)
  const [kbd, setKbd] = useState(autoFocus);

  const out = (v: string) => (!!min && v < min) || (!!max && v > max);
  const first = new Date(view.y, view.m, 1).getDay();
  const days = new Date(view.y, view.m + 1, 0).getDate();
  const move = (dm: number) => setView(v => {
    const d = new Date(v.y, v.m + dm, 1);
    return { y: d.getFullYear(), m: d.getMonth() };
  });

  useEffect(() => {
    if (mode !== 'day' || !kbd) return;
    gridRef.current?.querySelector<HTMLElement>(`[data-day="${focus}"]`)?.focus({ preventScroll: true });
  }, [focus, mode, view, kbd]);
  useEffect(() => {
    if (mode !== 'year') return;
    const box = yearsRef.current, on = box?.querySelector<HTMLElement>('.is-on');
    if (box && on) box.scrollTop = Math.max(0, on.offsetTop - box.clientHeight / 2 + on.clientHeight / 2);
  }, [mode]);

  useTopKeys(e => {
    if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); onClose(); return; }
    if (mode !== 'day' || !gridRef.current?.contains(document.activeElement)) return;
    setKbd(true);
    const step = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -7, ArrowDown: 7 }[e.key];
    if (step == null) return;
    e.preventDefault(); e.stopPropagation();
    const p = parseIso(focus)!;
    const d = new Date(p.y, p.m, p.d + step);
    setView({ y: d.getFullYear(), m: d.getMonth() });
    setFocus(iso(d.getFullYear(), d.getMonth(), d.getDate()));
  });

  const minY = parseIso(min)?.y ?? 1920;
  const maxY = parseIso(max)?.y ?? Math.max(2040, new Date().getFullYear() + 10);
  // 작은 해부터 — 발매일처럼 올해부터 고르는 칸은 올해가 맨 앞에 온다
  const years: number[] = [];
  for (let y = minY; y <= maxY; y++) years.push(y);
  const monthOut = (y: number, m: number) =>
    (!!min && `${y}-${pad(m + 1)}` < min.slice(0, 7)) || (!!max && `${y}-${pad(m + 1)}` > max.slice(0, 7));

  return (
    <div className="aq-cal">
      <div className="aq-cal-head">
        <button type="button" className="aq-cal-title" onClick={() => setMode(mode === 'day' ? 'year' : mode === 'month' ? 'year' : 'day')} aria-expanded={mode !== 'day'}>
          {mode === 'day' ? `${view.y}년 ${view.m + 1}월` : mode === 'month' ? `${view.y}년` : '연도 선택'}
          <Glyph name="chevron-down" size={16} />
        </button>
        {mode === 'day' && (
          <span className="aq-cal-nav">
            <button type="button" aria-label="이전 달" onClick={() => move(-1)}><Glyph name="chevron-left" size={18} /></button>
            <button type="button" aria-label="다음 달" onClick={() => move(1)}><Glyph name="chevron-right" size={18} /></button>
          </span>
        )}
      </div>

      {mode === 'year' ? (
        <div className="aq-cal-years" ref={yearsRef} role="listbox" aria-label={`${title} 연도`}>
          {years.map(y => (
            <button key={y} type="button" role="option" aria-selected={y === view.y} className={y === view.y ? 'is-on' : ''}
              onClick={() => { setView(v => ({ y, m: v.m })); setMode('month'); }}>
              {y}
            </button>
          ))}
        </div>
      ) : mode === 'month' ? (
        <div className="aq-cal-months" role="listbox" aria-label={`${title} ${view.y}년 월`}>
          {Array.from({ length: 12 }, (_, m) => (
            <button key={m} type="button" role="option" aria-selected={m === view.m} className={m === view.m ? 'is-on' : undefined}
              disabled={monthOut(view.y, m)} onClick={() => { setView({ y: view.y, m }); setMode('day'); }}>
              {m + 1}월
            </button>
          ))}
        </div>
      ) : (
        <>
          <div className="aq-cal-week" aria-hidden="true">
            {WEEK.map((w, i) => <span key={w} className={i === 0 ? 'is-sun' : undefined}>{w}</span>)}
          </div>
          <div className="aq-cal-grid" ref={gridRef} role="grid" aria-label={`${view.y}년 ${view.m + 1}월`}>
            {Array.from({ length: first }, (_, i) => <span key={`b${i}`} aria-hidden="true" />)}
            {Array.from({ length: days }, (_, i) => {
              const v = iso(view.y, view.m, i + 1);
              const cls = [v === value ? 'is-on' : '', v === today ? 'is-today' : ''].filter(Boolean).join(' ');
              return (
                <button
                  key={v} type="button" data-day={v} className={cls || undefined} disabled={out(v)}
                  tabIndex={v === focus ? 0 : -1} aria-pressed={v === value} aria-label={formatDateKo(v)}
                  onClick={() => onPick(v)}
                >
                  {i + 1}
                </button>
              );
            })}
          </div>
        </>
      )}

      {foot && <div className="aq-cal-foot">
        {!out(today) && <button type="button" className="ui-btn is-soft is-sm" onClick={() => onPick(today)}>오늘</button>}
        {!required && value && <button type="button" className="ui-btn is-white is-sm" onClick={onClear}>지우기</button>}
      </div>}
    </div>
  );
}

// ---------------------------------------------------------------------------
// 칸 껍데기 — 읽기 전용 입력칸(다른 입력칸과 같은 모양) + 누르면 열리는 AUDENIQ 창
// ---------------------------------------------------------------------------
type FieldBase = {
  id?: string; required?: boolean; placeholder?: string; className?: string; disabled?: boolean;
  'aria-label'?: string; 'aria-invalid'?: boolean;
};
type OpenState = { viaTouch: boolean; kbd: boolean; rect: DOMRect; title: string };

function PickerInput({ id, display, placeholder, className, disabled, required, icon, kind, onClear, aria, children }: {
  id?: string; display: string; placeholder: string; className?: string; disabled?: boolean; required?: boolean;
  icon: 'calendar' | 'clock'; kind: string; onClear?: () => void; aria: Pick<FieldBase, 'aria-label' | 'aria-invalid'>;
  children: (close: () => void, open: OpenState) => ReactNode;
}) {
  const ref = useRef<HTMLInputElement>(null);
  const [open, setOpen] = useState<OpenState | null>(null);
  const touched = useRef(false);
  const close = useCallback(() => {
    const via = open?.viaTouch;
    setOpen(null);
    if (!via) ref.current?.focus({ preventScroll: true });
  }, [open]);
  const show = (el: HTMLInputElement, kbd: boolean) => {
    if (disabled) return;
    setOpen({ viaTouch: !kbd && touched.current, kbd, rect: el.getBoundingClientRect(), title: fieldTitle(el) });
    touched.current = false;
  };
  return (
    <>
      <input
        ref={ref} id={id} type="text" readOnly inputMode="none" autoComplete="off"
        className={`ui-input aq-date-input${icon === 'clock' ? ' is-time' : ''}${className ? ` ${className}` : ''}`}
        value={display} placeholder={placeholder} required={required} disabled={disabled}
        aria-haspopup="dialog" aria-expanded={!!open} {...aria}
        onTouchStart={() => { touched.current = true; }}
        onClick={e => show(e.currentTarget, false)}
        onKeyDown={e => {
          if (e.key === 'Enter' || e.key === ' ' || e.key === 'ArrowDown') { e.preventDefault(); show(e.currentTarget, true); }
          if ((e.key === 'Backspace' || e.key === 'Delete') && !required && display && onClear) { e.preventDefault(); onClear(); }
        }}
      />
      {open && (
        <Surface anchor={open.rect} title={open.title} onClose={close} className={kind}>
          {children(close, open)}
        </Surface>
      )}
    </>
  );
}

/** 날짜 칸 — 값은 'YYYY-MM-DD', 화면에는 '2026년 10월 30일'. 누르면 AUDENIQ 달력이 열린다. */
export function DateField({ value, onChange, min, max, placeholder = '날짜를 선택해 주세요', ...rest }: FieldBase & {
  value: string; onChange: (v: string) => void; min?: string; max?: string;
}) {
  const { 'aria-label': al, 'aria-invalid': ai, ...base } = rest;
  return (
    <PickerInput {...base} aria={{ 'aria-label': al, 'aria-invalid': ai }} icon="calendar" kind="is-calendar"
      display={formatDateKo(value)} placeholder={placeholder} onClear={() => onChange('')}>
      {(close, open) => (
        <Calendar
          value={value} min={min} max={max} required={rest.required} title={open.title} autoFocus={open.kbd}
          onPick={v => { onChange(v); close(); }}
          onClear={() => { onChange(''); close(); }}
          onClose={close}
        />
      )}
    </PickerInput>
  );
}

// ---------------------------------------------------------------------------
// 시간 — 오전/오후 · 시 · 분을 눌러 고르는 세 줄 (휠처럼 돌리지 않고 누른다)
// ---------------------------------------------------------------------------
export const formatTimeKo = (v: string) => {
  const m = /^(\d{2}):(\d{2})/.exec(v ?? '');
  if (!m) return '';
  const h = +m[1];
  return `${h < 12 ? '오전' : '오후'} ${h % 12 === 0 ? 12 : h % 12}:${m[2]}`;
};

function TimeColumns({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const m = /^(\d{2}):(\d{2})/.exec(value) ?? ['', '09', '00'];
  const h = +m[1], min = +m[2];
  const pm = h >= 12, h12 = h % 12 === 0 ? 12 : h % 12;
  const minutes = Array.from({ length: 12 }, (_, i) => i * 5);
  if (!minutes.includes(min)) minutes.push(min), minutes.sort((a, b) => a - b);
  const set = (nh: number, nm: number) => onChange(`${pad(nh)}:${pad(nm)}`);
  const to24 = (v12: number, isPm: boolean) => (v12 % 12) + (isPm ? 12 : 0);
  const boxRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    boxRef.current?.querySelectorAll<HTMLElement>('.aq-time-col .is-on').forEach(el => {
      const col = el.parentElement!;
      col.scrollTop = el.offsetTop - col.clientHeight / 2 + el.clientHeight / 2;
    });
  }, []);
  return (
    <div className="aq-time" ref={boxRef}>
      <div className="aq-time-col" role="listbox" aria-label="오전·오후">
        {[false, true].map(isPm => (
          <button key={String(isPm)} type="button" role="option" aria-selected={pm === isPm} className={pm === isPm ? 'is-on' : undefined}
            onClick={() => set(to24(h12, isPm), min)}>{isPm ? '오후' : '오전'}</button>
        ))}
      </div>
      <div className="aq-time-col" role="listbox" aria-label="시">
        {Array.from({ length: 12 }, (_, i) => i + 1).map(v => (
          <button key={v} type="button" role="option" aria-selected={v === h12} className={v === h12 ? 'is-on' : undefined}
            onClick={() => set(to24(v, pm), min)}>{v}시</button>
        ))}
      </div>
      <div className="aq-time-col" role="listbox" aria-label="분">
        {minutes.map(v => (
          <button key={v} type="button" role="option" aria-selected={v === min} className={v === min ? 'is-on' : undefined}
            onClick={() => set(h, v)}>{pad(v)}분</button>
        ))}
      </div>
    </div>
  );
}

function TimeSheet({ value, onDone, onClose }: { value: string; onDone: (v: string) => void; onClose: () => void }) {
  const [t, setT] = useState(value || '09:00');
  useTopKeys(e => { if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); onClose(); } });
  return (
    <div className="aq-cal">
      <TimeColumns value={t} onChange={setT} />
      <div className="aq-cal-foot"><button type="button" className="ui-btn is-primary is-sm aq-cal-done" onClick={() => onDone(t)}>확인</button></div>
    </div>
  );
}

/** 시간 칸 — 값은 'HH:mm', 화면에는 '오전 9:00' */
export function TimeField({ value, onChange, placeholder = '시간을 선택해 주세요', ...rest }: FieldBase & {
  value: string; onChange: (v: string) => void;
}) {
  const { 'aria-label': al, 'aria-invalid': ai, ...base } = rest;
  return (
    <PickerInput {...base} aria={{ 'aria-label': al, 'aria-invalid': ai }} icon="clock" kind="is-time"
      display={formatTimeKo(value)} placeholder={placeholder} onClear={() => onChange('')}>
      {close => <TimeSheet value={value} onClose={close} onDone={v => { onChange(v); close(); }} />}
    </PickerInput>
  );
}

// ---------------------------------------------------------------------------
// 날짜 + 시간 — 값은 'YYYY-MM-DDTHH:mm'(datetime-local과 같은 모양)
// ---------------------------------------------------------------------------
export const formatDateTimeKo = (v: string) => {
  const [d, t] = (v ?? '').split('T');
  const ds = formatDateKo(d ?? '');
  return ds ? `${ds} ${formatTimeKo(t ?? '')}`.trim() : '';
};

function DateTimeSheet({ value, min, required, title, kbd, onDone, onClear, onClose }: {
  value: string; min?: string; required?: boolean; title: string; kbd: boolean;
  onDone: (v: string) => void; onClear: () => void; onClose: () => void;
}) {
  const [d0, t0] = (value ?? '').split('T');
  const [date, setDate] = useState(d0 ?? '');
  const [time, setTime] = useState((t0 ?? '').slice(0, 5) || '09:00');
  return (
    <div className="aq-dt">
      <Calendar
        value={date} min={min?.slice(0, 10)} required title={title} autoFocus={kbd} foot={false}
        onPick={setDate} onClear={() => setDate('')} onClose={onClose}
      />
      <TimeColumns value={time} onChange={setTime} />
      <div className="aq-cal-foot">
        {!required && value && <button type="button" className="ui-btn is-white is-sm" onClick={onClear}>지우기</button>}
        <button type="button" className="ui-btn is-primary is-sm aq-cal-done" disabled={!date} onClick={() => onDone(`${date}T${time}`)}>
          {date ? `${formatDateKo(date)} ${formatTimeKo(time)}` : '날짜를 골라 주세요'}
        </button>
      </div>
    </div>
  );
}

/** 날짜·시간 칸 — `<input type="datetime-local">` 대신 */
export function DateTimeField({ value, onChange, min, placeholder = '날짜와 시간을 선택해 주세요', ...rest }: FieldBase & {
  value: string; onChange: (v: string) => void; min?: string;
}) {
  const { 'aria-label': al, 'aria-invalid': ai, ...base } = rest;
  return (
    <PickerInput {...base} aria={{ 'aria-label': al, 'aria-invalid': ai }} icon="calendar" kind="is-calendar is-datetime"
      display={formatDateTimeKo(value)} placeholder={placeholder} onClear={() => onChange('')}>
      {(close, open) => (
        <DateTimeSheet
          value={value} min={min} required={rest.required} title={open.title} kbd={open.kbd}
          onDone={v => { onChange(v); close(); }} onClear={() => { onChange(''); close(); }} onClose={close}
        />
      )}
    </PickerInput>
  );
}

// ---------------------------------------------------------------------------
// 달(月) — 값은 'YYYY-MM'
// ---------------------------------------------------------------------------
export const formatMonthKo = (v: string) => {
  const m = /^(\d{4})-(\d{2})$/.exec(v ?? '');
  return m ? `${m[1]}년 ${+m[2]}월` : '';
};

function MonthSheet({ value, min, max, onPick, onClose }: { value: string; min?: string; max?: string; onPick: (v: string) => void; onClose: () => void }) {
  const now = new Date();
  const [year, setYear] = useState(+(value || max || '').slice(0, 4) || now.getFullYear());
  useTopKeys(e => { if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); onClose(); } });
  const key = (m: number) => `${year}-${pad(m + 1)}`;
  return (
    <div className="aq-cal">
      <div className="aq-cal-head">
        <span className="aq-cal-title is-static">{year}년</span>
        <span className="aq-cal-nav">
          <button type="button" aria-label="이전 해" disabled={!!min && `${year - 1}-12` < min} onClick={() => setYear(y => y - 1)}><Glyph name="chevron-left" size={18} /></button>
          <button type="button" aria-label="다음 해" disabled={!!max && `${year + 1}-01` > max} onClick={() => setYear(y => y + 1)}><Glyph name="chevron-right" size={18} /></button>
        </span>
      </div>
      <div className="aq-cal-months">
        {Array.from({ length: 12 }, (_, m) => {
          const k = key(m);
          return (
            <button key={k} type="button" className={k === value ? 'is-on' : undefined} aria-pressed={k === value}
              disabled={(!!min && k < min) || (!!max && k > max)} onClick={() => onPick(k)}>
              {m + 1}월
            </button>
          );
        })}
      </div>
    </div>
  );
}

/** 달 칸 — `<input type="month">` 대신. 화면에는 '2026년 10월' */
export function MonthField({ value, onChange, min, max, placeholder = '달을 선택해 주세요', ...rest }: FieldBase & {
  value: string; onChange: (v: string) => void; min?: string; max?: string;
}) {
  const { 'aria-label': al, 'aria-invalid': ai, ...base } = rest;
  return (
    <PickerInput {...base} aria={{ 'aria-label': al, 'aria-invalid': ai }} icon="calendar" kind="is-calendar"
      display={formatMonthKo(value)} placeholder={placeholder} onClear={() => onChange('')}>
      {close => <MonthSheet value={value} min={min} max={max} onClose={close} onPick={v => { onChange(v); close(); }} />}
    </PickerInput>
  );
}
