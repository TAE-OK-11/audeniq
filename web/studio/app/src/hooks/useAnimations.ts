import { useEffect, useRef } from 'react';

/**
 * 요소가 화면에 보일 때 자식 막대들에 한 번만 grow 클래스를 추가하는 훅.
 * 차트 막대 자라나기 등 스크롤 트리거 애니메이션용.
 * @param barSelector 자식 막대들의 셀렉터 (기본: .report-bar)
 */
export function useGrowOnView<T extends HTMLElement>(barSelector: string = '.report-bar') {
  const ref = useRef<T>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;

    const io = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) {
            el.querySelectorAll(barSelector).forEach((bar) =>
              bar.classList.add('grow')
            );
            io.disconnect();
          }
        }
      },
      { threshold: 0.3 }
    );

    io.observe(el);
    return () => io.disconnect();
  }, [barSelector]);

  return ref;
}

/**
 * 위자드 진행바: 다음 단계로 넘어갈 때 새 구간이 왼쪽에서 차오르는 애니메이션.
 * @param step 현재 단계 인덱스
 * @returns 진행바 컨테이너에 연결할 ref
 */
export function useProgressFill(step: number) {
  const ref = useRef<HTMLDivElement>(null);
  const prevStepRef = useRef(step);

  useEffect(() => {
    const el = ref.current;
    const prevStep = prevStepRef.current;
    prevStepRef.current = step;

    // 앞으로 이동할 때만 차오르는 애니메이션
    if (step <= prevStep || !el) return;
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;

    const spans = el.querySelectorAll('span');
    const newSpan = spans[step];
    if (!newSpan) return;

    // CSS의 transform-origin:left center와 함께 왼쪽에서 차오름
    newSpan.animate(
      [{ transform: 'scaleX(0)' }, { transform: 'scaleX(1)' }],
      { duration: 280, easing: 'cubic-bezier(.22,1,.36,1)' }
    );
  }, [step]);

  return ref;
}

/** CSS 커스텀 프로퍼티(--h 등)를 포함한 스타일 타입 */
export interface CSSVarStyle extends React.CSSProperties {
  [key: `--${string}`]: string | number | undefined;
}
