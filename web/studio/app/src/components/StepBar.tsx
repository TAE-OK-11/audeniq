// 여러 단계 흐름의 머리 — 칸 막대 + 'n단계 · 이름' 한 줄 (수익 받을 정보 등록 흐름과 같은 모양)
export function StepBar({ steps, current }: { steps: string[]; current: number }) {
  return (
    <div className="aq-stepbar">
      <div className="aq-stepbar-track" role="progressbar" aria-valuemin={1} aria-valuemax={steps.length} aria-valuenow={current + 1} aria-label={`${steps.length}단계 중 ${current + 1}단계`}>
        {steps.map((s, i) => <span key={s} className={i <= current ? 'is-on' : ''} />)}
      </div>
      <p className="aq-stepbar-label"><b>{current + 1}단계</b> · {steps[current]}</p>
    </div>
  );
}
