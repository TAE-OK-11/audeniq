// 고르기 토글(세그먼트)의 흰 알약이 고른 칸으로 미끄러지게 — 고른 버튼 자리를 재서 CSS 변수로 넘긴다.
// memoryKey를 주면 화면이 바뀌어 다시 그려져도(하위 탭 링크) 이전 칸에서 출발해 미끄러진다.
import { useLayoutEffect, useRef, useState, type CSSProperties } from 'react';

type Pos = { x: number; y: number; w: number; h: number };
const lastPos = new Map<string, Pos>();
const ACTIVE = '[aria-checked="true"],[aria-selected="true"]';

export function useSegPill<T extends HTMLElement = HTMLDivElement>(active: unknown, memoryKey?: string) {
  const ref = useRef<T>(null);
  const [pos, setPos] = useState<Pos | null>(() => (memoryKey && lastPos.get(memoryKey)) || null);
  const [live, setLive] = useState(false);

  useLayoutEffect(() => {
    const box = ref.current;
    if (!box) return;
    const measure = () => {
      const on = box.querySelector<HTMLElement>(ACTIVE);
      if (!on) { setPos(null); return; }
      const next = { x: on.offsetLeft, y: on.offsetTop, w: on.offsetWidth, h: on.offsetHeight };
      if (memoryKey) lastPos.set(memoryKey, next);
      setPos(p => (p && p.x === next.x && p.y === next.y && p.w === next.w && p.h === next.h ? p : next));
    };
    // 처음 그릴 때는 움직임 없이 제자리, 그다음 프레임부터 미끄러진다 (기억한 자리가 있으면 거기서 출발)
    let raf = 0;
    if (live) measure();
    else {
      const remembered = memoryKey && lastPos.has(memoryKey);
      if (!remembered) measure();
      raf = requestAnimationFrame(() => { setLive(true); if (remembered) measure(); });
    }
    const ro = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measure);
    ro?.observe(box);
    return () => { cancelAnimationFrame(raf); ro?.disconnect(); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, live]);

  const style = {
    '--pill-x': `${pos?.x ?? 0}px`,
    '--pill-y': `${pos?.y ?? 0}px`,
    '--pill-w': `${pos?.w ?? 0}px`,
    '--pill-h': `${pos?.h ?? 0}px`,
    opacity: pos ? 1 : 0,
  } as CSSProperties;
  return { ref, live, pillStyle: style };
}
