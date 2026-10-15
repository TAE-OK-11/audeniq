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

/** iOS Safari: 키보드가 올라오면 '보이는 영역'(visual viewport)만 줄어들고 그 안에서 화면이 밀린다.
 *  고정된 배경·아래 시트는 원래 화면 기준이라 시트가 중간에 뜨고, 그 아래로 어둡게 덮이지 않은 뒤 화면이 비쳤다.
 *  키보드가 떠 있는 동안 배경을 지금 보이는 영역에 맞춰 시트가 키보드 바로 위에 붙게 한다. 키보드가 내려가면 원래대로. */
export function fitToVisualViewport(el: HTMLElement, vv: VisualViewport, layoutHeight: number) {
  const shifted = vv.offsetTop > 0.5 || vv.height < layoutHeight - 0.5;
  if (shifted) {
    el.style.setProperty('top', `${vv.offsetTop}px`, 'important');
    el.style.setProperty('bottom', 'auto', 'important');
    // 배경은 보이는 영역 위에서부터 키보드 아래까지 넉넉히 덮는다 — 키보드 위 Safari 주소창·자동완성 막대는 반투명이라,
    // 보이는 영역에 딱 맞추면 그 막대 뒤로 어둡게 덮이지 않은 화면이 비쳤다. 시트는 --aq-kb로 따로 키보드 위에 붙는다
    el.style.setProperty('height', `${vv.height + layoutHeight}px`, 'important');
    el.style.setProperty('--aq-vv-h', `${vv.height}px`);
    el.style.setProperty('--aq-kb', `${Math.max(0, layoutHeight - vv.offsetTop - vv.height)}px`);
  } else {
    el.style.removeProperty('top');
    el.style.removeProperty('bottom');
    el.style.removeProperty('height');
    el.style.removeProperty('--aq-vv-h');
    el.style.removeProperty('--aq-kb');
  }
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
