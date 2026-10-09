import { BANK_SYMBOLS } from '../lib/bankSymbols';

// 가로로 긴 글자 로고 — 정사각 칸에 넣으면 너무 작아져 기관 카드에서 가로 폭을 더 준다
const WIDE = new Set(['miraeasset.svg', 'samsung.svg']);

export function BankLogo({ name }: { name: string }) {
  const file = BANK_SYMBOLS[name] ?? 'institution.svg';
  const src = `${import.meta.env.BASE_URL}static/banks/${file}`;
  return (
    <span className={`aq-bank-logo is-symbol${WIDE.has(file) ? ' is-wide' : ''}`} aria-hidden="true">
      <img
        key={src}
        src={src}
        alt=""
        decoding="async"
        draggable={false}
        onError={event => {
          const image = event.currentTarget;
          const fallback = `${import.meta.env.BASE_URL}static/banks/institution.svg`;
          if (image.getAttribute('src') !== fallback) image.src = fallback;
        }}
      />
    </span>
  );
}
