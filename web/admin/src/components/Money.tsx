// 금액 표시 — 숫자는 자리 맞춤, 단위(원·달러…)는 작고 옅게. mark를 켜면 화폐 기호를 AUDENIQ 아이콘으로 앞에 붙인다.
// 글꼴·OS마다 모양이 다른 ₩ $ € ¥ £ 글자는 쓰지 않는다. 화면 읽기 프로그램에는 ‘12,000원’처럼 읽힌다.
import { Glyph } from './Glyph';

const UNIT: Record<string, { unit: string; mark: string; digits: number }> = {
  KRW: { unit: '원', mark: 'krw', digits: 0 },
  USD: { unit: '달러', mark: 'usd', digits: 2 },
  EUR: { unit: '유로', mark: 'eur', digits: 2 },
  JPY: { unit: '엔', mark: 'jpy', digits: 0 },
  GBP: { unit: '파운드', mark: 'gbp', digits: 2 },
};

export function Money({ value, currency = 'KRW', mark = false, className = '' }: { value: number | string; currency?: string; mark?: boolean; className?: string }) {
  const n = Number(value);
  const v = Number.isFinite(n) ? n : 0;
  const c = UNIT[currency] ?? { unit: currency, mark: '', digits: 2 };
  const num = new Intl.NumberFormat('ko-KR', { maximumFractionDigits: c.digits }).format(c.digits ? v : Math.round(v));
  const label = `${num}${c.unit}`;
  return (
    <span className={`aq-money${mark && c.mark ? ' is-mark' : ''}${v < 0 ? ' is-minus' : ''}${className ? ` ${className}` : ''}`} role="text" aria-label={label}>
      {mark && c.mark && <span className="aq-money-mark" aria-hidden="true"><Glyph name={c.mark} size={16} /></span>}
      <span className="aq-money-num" aria-hidden="true">{num}</span>
      <span className="aq-money-unit" aria-hidden="true">{c.unit}</span>
    </span>
  );
}
