// 심사 시트 바로가기 — 긴 시트를 위아래로 오가지 않도록 섹션 이름을 가로로 늘어놓고 지금 보는 곳을 표시한다.
// 결정 칸이 옆에 붙지 않는 폭(≤1180px)에서만 보인다 (styles: .adm-jump).
import { useEffect, useRef, useState } from 'react';

export interface JumpItem { id: string; label: string; count?: number; tone?: 'bad' | 'warn' }

export function JumpNav({ items }: { items: JumpItem[] }) {
  const [active, setActive] = useState(items[0]?.id ?? '');
  const bar = useRef<HTMLElement>(null);
  const key = items.map(i => i.id).join('|');

  // 지금 보는 섹션: 바로가기 바 바로 아래(화면 위 ~160px)를 지난 마지막 섹션. 바닥에 닿으면 마지막 섹션
  const lock = useRef(0);
  useEffect(() => {
    const ids = key.split('|');
    let raf = 0;
    const update = () => {
      raf = 0;
      if (Date.now() < lock.current) return;
      const line = (bar.current?.getBoundingClientRect().bottom ?? 0) + 24;
      let current = ids[0];
      for (const id of ids) {
        const el = document.getElementById(id);
        if (el && el.offsetHeight > 0 && el.getBoundingClientRect().top <= line) current = id;
      }
      if (window.innerHeight + window.scrollY >= document.documentElement.scrollHeight - 4) current = ids[ids.length - 1];
      setActive(current);
    };
    const onScroll = () => { if (!raf) raf = requestAnimationFrame(update); };
    window.addEventListener('scroll', onScroll, { passive: true });
    update();
    return () => { window.removeEventListener('scroll', onScroll); if (raf) cancelAnimationFrame(raf); };
  }, [key]);

  // 지금 섹션 버튼이 바 안에서 보이도록 (바만 가로로 움직인다)
  useEffect(() => {
    const btn = bar.current?.querySelector<HTMLElement>(`[data-id="${active}"]`);
    const nav = bar.current;
    if (!btn || !nav) return;
    const left = btn.offsetLeft - nav.clientWidth / 2 + btn.clientWidth / 2;
    nav.scrollTo({ left: Math.max(0, left), behavior: 'smooth' });
  }, [active]);

  const go = (id: string) => {
    const el = document.getElementById(id);
    if (!el) return;
    setActive(id);
    lock.current = Date.now() + 900; // 부드러운 스크롤 중에는 표시가 앞 섹션으로 튀지 않게
    const reduce = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;
    el.scrollIntoView({ behavior: reduce ? 'auto' : 'smooth', block: 'start' });
  };

  if (items.length < 2) return null;
  return (
    <nav ref={bar} className="adm-jump" aria-label="심사 시트 바로가기">
      {items.map(i => (
        <button key={i.id} type="button" data-id={i.id} className={active === i.id ? 'is-on' : ''} aria-current={active === i.id ? 'true' : undefined} onClick={() => go(i.id)}>
          {i.label}
          {i.count != null && <span className={i.tone ? `is-${i.tone}` : ''}>{i.count}</span>}
        </button>
      ))}
    </nav>
  );
}
