import { BANK_SYMBOLS } from '../lib/bankSymbols';

// 배경까지 꽉 찬 사각 로고 — 원형 칸에서는 여백 없이 채운다 (값: 파일 안쪽 여백을 걷어내는 배율)
const FILLED: Record<string, number> = { 'miraeasset.svg': 1, 'samsung.svg': 1, 'kakao.svg': 1.36 };

export function BankLogo({ name }: { name: string }) {
  const file = BANK_SYMBOLS[name] ?? 'institution.svg';
  const src = `${import.meta.env.BASE_URL}static/banks/${file}`;
  return (
    <span
      className={`aq-bank-logo is-symbol${FILLED[file] ? ' is-fill' : ''}`}
      style={FILLED[file] ? { ['--logo-fill' as string]: FILLED[file] } : undefined}
      aria-hidden="true"
    >
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
