// 발매 신청서 — 작은 화면 조각
import { DateField } from '../../components/Picker';


/** 플랫폼 이름은 ‘·’ 뒤에서만 줄을 바꾼다 — 좁은 칸에서 ‘Instagram·Fa / cebook’처럼 단어 중간이 끊기던 것 */
export const breakAtDots = (label: string) =>
  label.split('·').flatMap((part, i): React.ReactNode[] => (i ? [<span key={i}>·<wbr />{part}</span>] : [part]));

export function KoreanDateField({ id, label, required, value, min, onChange, help }: {
  id: string; label: React.ReactNode; required?: boolean;
  value: string; min?: string; onChange: (v: string) => void; help?: React.ReactNode;
}) {
  return (
    <div className="ui-field">
      <label htmlFor={id}>{label}{required && <> <span className="required">*</span></>}</label>
      <DateField id={id} value={value} min={min} required={required} onChange={onChange} />
      {help && <small className="ui-field-help">{help}</small>}
    </div>
  );
}

export function BackIcon() {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" width="24" height="24" fill="none"
      stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="m15 18-6-6 6-6" />
    </svg>
  );
}
