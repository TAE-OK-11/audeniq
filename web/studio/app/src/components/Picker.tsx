// AUDENIQ 선택 시트 · 달력 — 기기마다 모양이 다른 기본 선택 팝업(iOS 휠, 안드로이드 다이얼로그, 데스크톱 목록)과
// 기본 달력 대신 어디서나 같은 모양으로 보여 준다. 선택 목록은 어느 기기든 칸 바로 아래 작은 창,
// 달력·시간은 휴대폰에서 아래에서 올라오는 시트(넓은 화면은 칸 아래 작은 창).
//
// 공통 바탕과 <SelectHost/>는 PickerSurface.tsx (앱 첫 번들).
import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { Glyph } from './Glyph';
import { Surface, fieldTitle, useTopKeys } from './PickerSurface';
export { SelectHost } from './PickerSurface';
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
