// 진행 단계 카드 — 지금 단계 이름·안내를 머리에, 아래는 칸 막대(지난 단계는 체크). 발매 상세와 문의(플랫폼 요청)가 같은 모양을 쓴다.
// 스타일: styles/enhance.css .aq-stage
import { CheckIcon } from './Check';

export function StageCard({ steps, current, complete, tone, note, label }: {
  steps: readonly string[];
  /** 지금 단계 (0부터) */
  current: number;
  /** 마지막 단계까지 끝났는지 (지금 단계도 체크로) */
  complete?: boolean;
  /** warn: 보완 필요 · error: 거절·처리 불가 */
  tone?: 'warn' | 'error';
  note?: string;
  label: string;
}) {
  return (
    <section className={`aq-stage${tone ? ` is-${tone}` : ''}`} aria-label={label}>
      <div className="aq-stage-head">
        <div className="min-0">
          <span className="aq-stage-kicker">진행 단계</span>
          <strong className="aq-stage-now">{steps[current]}</strong>
        </div>
        <span className="aq-stage-count" aria-label={`${steps.length}단계 중 ${current + 1}단계`}>
          {current + 1}<small>/ {steps.length}</small>
        </span>
      </div>
      {note && <p className="aq-stage-note">{note}</p>}
      <ol className="aq-stage-track" style={{ gridTemplateColumns: `repeat(${steps.length},minmax(0,1fr))` }}>
        {steps.map((s, i) => {
          const done = i < current || (i === current && !!complete);
          return (
            <li key={i} className={done ? 'is-done' : i === current ? 'is-current' : ''} aria-current={i === current ? 'step' : undefined}>
              <span className="aq-stage-bar" aria-hidden="true" />
              <span className="aq-stage-label">
                {done && <CheckIcon size={10} />}
                {s}
              </span>
            </li>
          );
        })}
      </ol>
    </section>
  );
}
