// 신청서 공용 선택 버튼 — 짧은 선택지는 칩, 문장처럼 긴 선택지는 한 줄씩(list). ‘기타’를 누르면 직접 입력 칸이 열린다
import { CheckIcon } from './Check';

export const OTHER = '기타';

/** 버튼으로 고르는 선택지 — 직접 타이핑은 ‘기타’를 눌렀을 때만 */
export function ChipPicker({ id, options, value, onChange, other, onOther, otherPlaceholder, single, list }: {
  id: string; options: string[]; value: string[]; onChange: (v: string[]) => void;
  other?: string; onOther?: (v: string) => void; otherPlaceholder?: string; single?: boolean;
  /** 문장처럼 긴 선택지는 한 줄에 하나씩, 고른 표시는 오른쪽에 */
  list?: boolean;
}) {
  const toggle = (opt: string) => {
    const on = value.includes(opt);
    onChange(single ? (on ? [] : [opt]) : on ? value.filter(v => v !== opt) : [...value, opt]);
  };
  const all = onOther ? [...options, OTHER] : options;
  if (list) {
    return (
      <div className="aq-chip-picker" id={id}>
        <div className="aq-pick-list" role={single ? 'radiogroup' : 'group'}>
          {all.map(opt => {
            const on = value.includes(opt);
            return (
              <button
                key={opt} type="button" className={`aq-pick${on ? ' is-on' : ''}`}
                role={single ? 'radio' : undefined} aria-checked={single ? on : undefined} aria-pressed={single ? undefined : on}
                onClick={() => toggle(opt)}
              >
                <span>{opt === OTHER ? '기타 (직접 입력)' : opt}</span>
                <span className={`aq-pick-mark${single ? ' is-radio' : ''}`} aria-hidden="true">{on && <CheckIcon size={12} />}</span>
              </button>
            );
          })}
        </div>
        {onOther && value.includes(OTHER) && (
          <input className="aq-chip-other" maxLength={300} value={other ?? ''} placeholder={otherPlaceholder} onChange={e => onOther(e.target.value)} autoFocus />
        )}
      </div>
    );
  }
  return (
    <div className="aq-chip-picker" id={id}>
      <div className="aq-chips" role="group">
        {all.map(opt => (
          <button key={opt} type="button" className={`aq-chip${value.includes(opt) ? ' is-on' : ''}`} aria-pressed={value.includes(opt)} onClick={() => toggle(opt)}>
            {opt === OTHER ? '기타 (직접 입력)' : opt}
          </button>
        ))}
      </div>
      {onOther && value.includes(OTHER) && (
        <input className="aq-chip-other" maxLength={300} value={other ?? ''} placeholder={otherPlaceholder} onChange={e => onOther(e.target.value)} autoFocus />
      )}
    </div>
  );
}
