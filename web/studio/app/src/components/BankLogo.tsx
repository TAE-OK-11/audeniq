import { BANK_SYMBOLS } from '../lib/bankSymbols';

export function BankLogo({ name }: { name: string }) {
  const file = BANK_SYMBOLS[name] ?? 'institution.svg';
  const src = `${import.meta.env.BASE_URL}static/banks/${file}`;
  return (
    <span className="aq-bank-logo is-symbol" aria-hidden="true">
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
