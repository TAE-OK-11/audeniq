import { createContext, useCallback, useContext, useEffect, useId, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { useToast } from './Toast';

interface ModalProps {
  title: string;
  onClose: () => void;
  children: ReactNode;
  /** #modal에 추가할 클래스 (예: aq-payout-setup-mode, aq-signature-mode) */
  modalClass?: string;
  /** 배경 클릭으로 닫기 (입력 중 실수로 닫히는 것을 막고 싶으면 false) */
  dismissible?: boolean;
  closeDisabled?: boolean;
}

/** 휴대폰 아래 시트를 이만큼 끌어내리면(또는 빠르게 튕기면) 닫는다 */
const DRAG_CLOSE_PX = 96;
/** 아래 시트가 아닌 창 — 전체 화면이거나 가운데 뜨는 확인 창 */
const NOT_SHEET_MODES = ['aq-payout-setup-mode', 'aq-signature-mode', 'aq-confirm-mode', 'aq-sysdialog-mode'];

const FOCUSABLE = 'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';
const EXIT_MS = 180;

// body 스크롤 잠금 카운터 — 중첩 모달에서 하나가 닫혀도 잠금 유지
let lockCount = 0;
let savedPadding = '';
/** 창을 열 때의 스크롤 위치 — iOS Safari는 창 안 입력칸에 커서가 가면 잠긴 뒤 화면까지 스크롤해 버린다. 닫으면 되돌린다 */
let savedY = 0;
function lockScroll() {
  lockCount += 1;
  if (lockCount === 1) {
    savedY = window.scrollY;
    // 스크롤바가 사라지며 레이아웃이 밀리는 것을 방지
    const gap = window.innerWidth - document.documentElement.clientWidth;
    savedPadding = document.body.style.paddingRight;
    if (gap > 0) document.body.style.paddingRight = `${gap}px`;
    // html만 잠근다(뷰포트 스크롤이 멈춘다). body까지 잠그면 body가 따로 스크롤 상자가 돼
    // 스크롤해 둔 화면에서 고정 머리글이 그만큼 위로 밀려 사라진다.
    document.documentElement.style.overflow = 'hidden';
  }
}
function unlockScroll() {
  lockCount = Math.max(0, lockCount - 1);
  if (lockCount === 0) {
    document.documentElement.style.overflow = '';
    document.body.style.paddingRight = savedPadding;
    if (Math.abs(window.scrollY - savedY) > 1) window.scrollTo({ top: savedY, behavior: 'instant' as ScrollBehavior });
  }
}

/** iOS Safari: 키보드가 올라오면 '보이는 영역'(visual viewport)만 줄고 그 안에서 화면이 밀리는 경우가 있다.
 *  그때 키보드에 가린 높이(--aq-kb)와 보이는 높이(--aq-vv-h)를 단다. 아래 시트는 바닥에 둔 채(키보드 뒤로 이어지게)
 *  내용만 그만큼 올리고, 보이는 높이 안에 들게 한다. 어두운 배경은 CSS에서 화면 위아래로 넉넉히 덮으므로 여기서 크기를 바꾸지 않는다. */
export function fitToVisualViewport(el: HTMLElement, vv: VisualViewport, layoutHeight: number) {
  const shifted = vv.offsetTop > 0.5 || vv.height < layoutHeight - 0.5;
  if (shifted) {
    el.style.setProperty('--aq-vv-h', `${vv.height}px`);
    el.style.setProperty('--aq-kb', `${Math.max(0, layoutHeight - vv.offsetTop - vv.height)}px`);
  } else {
    el.style.removeProperty('--aq-vv-h');
    el.style.removeProperty('--aq-kb');
  }
}

/** iOS 26 Safari(파이어폭스 등 iOS 브라우저 모두): 키보드가 뜨면 고정 레이어(어두운 배경·아래 시트)는 줄어든 화면 안에만 그리고,
 *  그 바깥(아래 주소창·자동완성 띠 뒤, 위 상태 표시줄 쪽)에는 페이지 자체만 그린다. 그래서 고정 레이어로는 그 띠를 덮을 수 없었다.
 *  창이 열려 있는 동안 페이지 안에 덮개를 깐다: 시트가 끝나는 자리부터 아래로 흰 바탕, 화면 위쪽 바깥은 어두운 배경 색.
 *  시트의 실제 위치와 페이지 스크롤을 따라가므로 iOS가 화면 크기를 어떻게 알려 주든 맞는다. 평소에는 화면 밖에 있어 보이지 않는다. */
export function coverOutsideFixedLayer(overlay: HTMLElement, sheet: HTMLElement) {
  const make = (bg: string) => {
    const d = document.createElement('div');
    d.setAttribute('aria-hidden', 'true');
    d.dataset.aqSheetCover = '';
    d.style.cssText = `position:absolute;left:0;right:0;z-index:105;pointer-events:none;background:${bg}`;
    document.body.appendChild(d);
    return d;
  };
  // 위쪽 덮개는 어두운 배경이 흰 페이지 위에 겹친 색과 같게 — 배경이 서서히 나타나는 중에도 맞도록 맞출 때마다 다시 읽는다
  const dim = () => {
    const m = getComputedStyle(overlay).backgroundColor.match(/[\d.]+/g)?.map(Number) ?? [];
    const a = m.length > 3 ? m[3] : 1;
    return m.length < 3 ? '' : `rgb(${m.slice(0, 3).map(c => Math.round(255 * (1 - a) + c * a)).join(',')})`;
  };
  const floor = make('#fff');
  const ceiling = make('');
  let raf = 0;
  const place = () => {
    raf = 0;
    const h = Math.max(window.innerHeight, window.screen?.height ?? 0);
    const y = window.scrollY;
    const r = sheet.getBoundingClientRect();
    floor.style.top = `${Math.round(y + r.bottom - 40)}px`;
    floor.style.height = `${h}px`;
    ceiling.style.top = `${Math.round(y - h)}px`;
    ceiling.style.background = dim();
    ceiling.style.height = `${h}px`;
  };
  const schedule = () => { if (!raf) raf = requestAnimationFrame(place); };
  place();
  // 시트가 올라오는 움직임·키보드가 뜨는 동안에도 따라가도록 처음 잠깐은 매 프레임 맞춘다
  let until = performance.now() + 900;
  const follow = () => { place(); if (performance.now() < until) raf = requestAnimationFrame(follow); else raf = 0; };
  const burst = () => { until = performance.now() + 600; if (!raf) raf = requestAnimationFrame(follow); };
  raf = requestAnimationFrame(follow);
  const vv = window.visualViewport;
  window.addEventListener('scroll', schedule, { passive: true });
  window.addEventListener('resize', burst);
  vv?.addEventListener('resize', burst);
  vv?.addEventListener('scroll', schedule);
  sheet.addEventListener('focusin', burst);
  const ro = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(schedule);
  ro?.observe(sheet);
  return () => {
    cancelAnimationFrame(raf);
    window.removeEventListener('scroll', schedule);
    window.removeEventListener('resize', burst);
    vv?.removeEventListener('resize', burst);
    vv?.removeEventListener('scroll', schedule);
    sheet.removeEventListener('focusin', burst);
    ro?.disconnect();
    floor.remove();
    ceiling.remove();
  };
}

/** 모달 안의 자식이 닫힘 애니메이션과 함께 모달을 닫을 수 있게 한다 */
const ModalCloseContext = createContext<() => void>(() => {});
export const useModalClose = () => useContext(ModalCloseContext);

// 가장 위의 모달만 Esc/Tab에 반응하도록 스택 관리
const stack: symbol[] = [];

export function Modal({ title, onClose, children, modalClass, dismissible = true, closeDisabled = false }: ModalProps) {
  const innerRef = useRef<HTMLElement>(null);
  const overlayRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const vv = window.visualViewport;
    const el = overlayRef.current;
    if (!vv || !el) return;
    const sync = () => fitToVisualViewport(el, vv, window.innerHeight);
    sync();
    vv.addEventListener('resize', sync);
    vv.addEventListener('scroll', sync);
    return () => { vv.removeEventListener('resize', sync); vv.removeEventListener('scroll', sync); };
  }, []);
  // 휴대폰 아래 시트: 고정 레이어가 그려지지 않는 화면 바깥 띠를 페이지 안 덮개로 메운다 (iOS 26)
  useEffect(() => {
    const overlay = overlayRef.current;
    const sheetEl = innerRef.current;
    if (!overlay || !sheetEl || !window.matchMedia?.('(max-width: 760px)').matches) return;
    if (NOT_SHEET_MODES.some(m => modalClass?.includes(m))) return;
    return coverOutsideFixedLayer(overlay, sheetEl);
  }, [modalClass]);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const closeDisabledRef = useRef(closeDisabled);
  closeDisabledRef.current = closeDisabled;
  const [leaving, setLeaving] = useState(false);
  const leavingRef = useRef(false);
  // 작성 중인 창 — 안의 칸에 무엇이든 입력하면 켜진다. 이때는 바깥 누름·Esc·끌어내리기로 닫지 않고 창이 튀어
  // '닫기 버튼으로 닫아 주세요'를 알린다(실수로 스치거나 끌어 입력이 날아가지 않게). 닫기·취소 버튼은 그대로 닫힌다.
  const dirtyRef = useRef(false);
  const titleId = useId();

  const requestClose = useCallback(() => {
    if (closeDisabledRef.current) return;
    if (leavingRef.current) return;
    leavingRef.current = true;
    if (window.matchMedia?.('(prefers-reduced-motion: reduce)').matches) {
      onCloseRef.current();
      return;
    }
    setLeaving(true);
    window.setTimeout(() => onCloseRef.current(), EXIT_MS);
  }, []);

  useEffect(() => {
    const token = Symbol('modal');
    stack.push(token);
    const onKey = (e: KeyboardEvent) => {
      if (stack[stack.length - 1] !== token) return;
      if (e.key === 'Escape') {
        e.stopPropagation();
        if (dirtyRef.current) bumpRef.current();
        else requestClose();
        return;
      }
      // 포커스 트랩: Tab이 모달 안에서만 순환
      if (e.key === 'Tab') {
        const root = innerRef.current;
        if (!root) return;
        const list = Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE))
          .filter(el => el.offsetParent !== null);
        if (list.length === 0) { e.preventDefault(); return; }
        const first = list[0];
        const last = list[list.length - 1];
        if (e.shiftKey && (document.activeElement === first || !root.contains(document.activeElement))) {
          e.preventDefault();
          last.focus();
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault();
          first.focus();
        }
      }
    };
    document.addEventListener('keydown', onKey);
    lockScroll();
    // 열릴 때 모달 컨테이너로 포커스 이동(스크린리더가 제목부터 읽도록), 닫힐 때 복원
    const prevFocus = document.activeElement as HTMLElement | null;
    const t = window.setTimeout(() => {
      const root = innerRef.current;
      const auto = root?.querySelector<HTMLElement>('[autofocus], [data-autofocus]');
      (auto ?? root)?.focus({ preventScroll: true });
    }, 0);
    return () => {
      window.clearTimeout(t);
      document.removeEventListener('keydown', onKey);
      const i = stack.indexOf(token);
      if (i >= 0) stack.splice(i, 1);
      unlockScroll();
      if (prevFocus && document.contains(prevFocus)) prevFocus.focus({ preventScroll: true });
    };
  }, [requestClose]);

  // 닫을 수 없는 창의 바깥을 누르면 아무 반응이 없는 대신 창이 살짝 튀어 '여기서 마쳐 주세요'를 알린다
  const [nudge, setNudge] = useState(false);
  const bump = () => {
    if (nudge) return;
    setNudge(true);
    window.setTimeout(() => setNudge(false), 320);
  };
  // 작성 중이라 닫지 않았을 때는 왜 안 닫히는지 한 번만 알려 준다
  const toast = useToast();
  const hinted = useRef(false);
  const guard = () => {
    bump();
    if (dirtyRef.current && !hinted.current) {
      hinted.current = true;
      toast('작성 중인 내용이 있어요. 닫으려면 닫기 버튼을 눌러 주세요.', 'info');
    }
  };
  const bumpRef = useRef(guard);
  bumpRef.current = guard;

  // 휴대폰 아래 시트: 위쪽 머리(손잡이)를 잡고 끌어내려 닫는다. 덜 끌면 제자리로 튀어 돌아간다.
  const sheet = useRef<{ y: number; t: number; dy: number } | null>(null);
  const isSheet = () => window.matchMedia?.('(max-width: 760px)').matches && !NOT_SHEET_MODES.some(m => modalClass?.includes(m));
  const onDragStart = (e: React.PointerEvent) => {
    if (closeDisabledRef.current || e.pointerType === 'mouse' || !isSheet()) return;
    if ((e.target as Element).closest('button,a,input,select,textarea')) return;
    sheet.current = { y: e.clientY, t: performance.now(), dy: 0 };
    (e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId);
  };
  const onDragMove = (e: React.PointerEvent) => {
    const d = sheet.current;
    const el = innerRef.current;
    if (!d || !el) return;
    d.dy = Math.max(0, e.clientY - d.y);
    // 위로는 끌리지 않고, 아래로는 손가락을 따라간다
    el.style.transition = 'none';
    el.style.animation = 'none';
    el.style.transform = `translate3d(0,${d.dy}px,0)`;
  };
  const onDragEnd = () => {
    const d = sheet.current;
    const el = innerRef.current;
    sheet.current = null;
    if (!d || !el) return;
    const speed = d.dy / Math.max(1, performance.now() - d.t); // px/ms
    el.style.transition = 'transform .28s cubic-bezier(.3,1.4,.55,1)';
    if ((d.dy > DRAG_CLOSE_PX || (d.dy > 24 && speed > 0.6)) && dirtyRef.current) {
      // 작성 중이면 끌어내려도 닫지 않고 제자리로 튀어 돌아간 뒤 한 번 튄다
      el.style.transform = '';
      window.setTimeout(guard, 200);
    } else if (d.dy > DRAG_CLOSE_PX || (d.dy > 24 && speed > 0.6)) {
      el.style.transition = 'transform .2s ease-in';
      el.style.transform = 'translate3d(0,100%,0)';
      leavingRef.current = true;
      window.setTimeout(() => onCloseRef.current(), 190);
    } else {
      el.style.transform = '';
    }
  };

  const cls = ['modal', modalClass, leaving ? 'is-leaving' : '', nudge ? 'is-nudge' : ''].filter(Boolean).join(' ');

  // body 바로 아래에 그린다 — 애니메이션(transform) 중인 부모 안에 있으면 position:fixed의 기준이
  // 화면이 아니라 부모가 되어 창이 밀리거나 다른 고정 요소에 가려지는 문제를 막는다
  return createPortal(
    <div
      id="modal"
      ref={overlayRef}
      className={cls}
      onMouseDown={e => {
        if (e.target !== e.currentTarget) return;
        if (dismissible && !dirtyRef.current) requestClose(); else guard();
      }}
    >
      <section
        className="modal-inner" ref={innerRef} tabIndex={-1}
        role="dialog" aria-modal="true" aria-labelledby={titleId}
      >
        <div className="modal-top" onPointerDown={onDragStart} onPointerMove={onDragMove} onPointerUp={onDragEnd} onPointerCancel={onDragEnd}>
          <h2 id={titleId}>{title}</h2>
          <button className="icon-button" type="button" aria-label="닫기" onClick={requestClose} disabled={closeDisabled}>
            <svg viewBox="0 0 24 24" width="20" height="20" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round"><path d="M6 6l12 12M18 6 6 18" /></svg>
          </button>
        </div>
        <div id="modalBody" onInput={e => { if ((e.target as Element).matches('input,textarea,select,[contenteditable]')) dirtyRef.current = true; }}>
          <ModalCloseContext.Provider value={requestClose}>{children}</ModalCloseContext.Provider>
        </div>
      </section>
    </div>,
    document.body,
  );
}
