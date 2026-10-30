// 축하 조각 — 큰 일을 마친 순간(발매 신청, 계약 체결)에만 완료 표시 둘레로 작은 조각이 한 번 퍼진다.
// 토스가 기념할 순간에 쓰는 폭죽을 AUDENIQ 색(파랑 계열 + 보라·주황 한 점씩)으로 작게. 자주 쓰면 의미가 흐려진다
const PIECES = Array.from({ length: 14 }, (_, i) => ({
  a: (360 / 14) * i + (i % 2 ? 9 : -6),
  d: 46 + ((i * 7) % 4) * 9,
  c: ['var(--aq-blue-500)', 'var(--aq-blue-300)', 'var(--aq-violet-500)', 'var(--aq-blue-400)', 'var(--aq-amber-500)', 'var(--aq-blue-200)', 'var(--aq-blue-450)'][i % 7],
  s: i % 3 === 0 ? 7 : i % 3 === 1 ? 5 : 6,
  r: i % 4 === 0,
}));

export function Burst() {
  return (
    <span className="aq-burst" aria-hidden="true">
      {PIECES.map((p, i) => (
        <i key={i} className={p.r ? 'is-bar' : undefined}
          style={{ ['--a' as string]: `${p.a}deg`, ['--d' as string]: `${p.d}px`, ['--c' as string]: p.c, ['--s' as string]: `${p.s}px` }} />
      ))}
    </span>
  );
}
