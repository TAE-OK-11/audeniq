// 그 자리에서 펼치기 — 버튼을 누르면 폼이 툭 바뀌지 않고 높이가 부드럽게 열린다 (0.32s, 위로 6px에서 내려오며 나타남).
// 버튼 쪽도 같은 컴포넌트로 감싸면 버튼은 접히고 폼은 펼쳐져, 카드 높이가 한 번에 튀지 않는다.
// 다 열리면 폼 아래가 화면 밖일 때 카드 머리가 보이는 데까지만 살짝 내려 준다. 닫으면 접힌 뒤에 내용을 지운다.
import { useEffect, useRef, useState, type ReactNode } from 'react';
import { cx } from './tone';

const DURATION = 340; // --aq-motion-long(.32s) + 프레임 여유
const reduced = () => typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;

/** 열린 내용이 화면 밖이면 보이게 — 머리글·고정 탭과 아래 막대(탭 바·사파리 주소 막대)에 가리지 않는 만큼만 움직인다 */
function bringIntoView(el: HTMLElement) {
  const card = (el.parentElement?.closest('section, .ui-card') as HTMLElement | null) ?? el;
  const top = Math.max(0, ...Array.from(document.querySelectorAll<HTMLElement>('.portal-header, .aq-detail-tabs'), x => x.getBoundingClientRect().bottom)) + 12;
  const bottom = window.innerHeight - 96;
  const c = card.getBoundingClientRect();
  const r = el.getBoundingClientRect();
  let dy = 0;
  if (c.top < top) dy = c.top - top;                                  // 접히면서 카드 머리가 위로 사라졌으면 다시 보이게
  else if (r.bottom > bottom) dy = Math.min(r.bottom - bottom, c.top - top); // 펼친 아래쪽이 가려졌으면, 카드 머리는 남기고
  if (Math.abs(dy) > 2) window.scrollBy({ top: dy, behavior: reduced() ? 'auto' : 'smooth' });
}

export function Reveal({ open, children, className, focus = 'panel' }: {
  open: boolean; children: ReactNode; className?: string;
  /** 다 열렸을 때 초점 — 폼은 칸이 아니라 묶음에(휴대폰 자판이 갑자기 올라오지 않게), 버튼 쪽은 그 버튼에 */
  focus?: 'panel' | 'first';
}) {
  const ref = useRef<HTMLDivElement>(null);
  const first = useRef(true);
  const [mounted, setMounted] = useState(open);
  const [expanded, setExpanded] = useState(open);
  const [settled, setSettled] = useState(open);

  useEffect(() => {
    if (first.current) { first.current = false; return; }
    setSettled(false);
    let raf = 0;
    if (open) {
      setMounted(true);
      // 닫힌 모양으로 한 번 그린 다음 열어야 전환이 보인다
      raf = requestAnimationFrame(() => { raf = requestAnimationFrame(() => setExpanded(true)); });
    } else {
      setExpanded(false);
    }
    const t = window.setTimeout(() => {
      if (!open) { setMounted(false); return; }
      setSettled(true);
      const el = ref.current;
      if (!el) return;
      // 누른 버튼이 접히며 초점이 사라졌을 때만 옮긴다 (다른 곳을 보고 있으면 건드리지 않는다)
      const active = document.activeElement;
      if (!active || active === document.body || !active.isConnected || active.closest('[inert]')) {
        const target = focus === 'first' ? el.querySelector<HTMLElement>('button:not(:disabled), a[href], input, select, textarea') : null;
        (target ?? el).focus({ preventScroll: true });
      }
      bringIntoView(el);
    }, reduced() ? 0 : DURATION);
    return () => { cancelAnimationFrame(raf); window.clearTimeout(t); };
  }, [open]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!mounted) return null;
  return (
    <div ref={ref} tabIndex={-1} inert={!open}
      className={cx('ui-reveal', expanded && 'is-open', settled && 'is-settled', className)}>
      <div className="ui-reveal-inner">{children}</div>
    </div>
  );
}
