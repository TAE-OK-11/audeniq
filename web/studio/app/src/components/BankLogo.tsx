import { BANK_SYMBOLS } from '../lib/bankSymbols';

// 배경까지 칠해진 사각 로고 — 기관 카드에서 모서리를 둥글려 앱 아이콘처럼 보이게
const SQUARE = new Set(['miraeasset.svg', 'samsung.svg']);

export function BankLogo({ name }: { name: string }) {
  const file = BANK_SYMBOLS[name] ?? 'institution.svg';
  const src = `${import.meta.env.BASE_URL}static/banks/${file}`;
  return (
    <span className={`aq-bank-logo is-symbol${SQUARE.has(file) ? ' is-square' : ''}`} aria-hidden="true">
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
