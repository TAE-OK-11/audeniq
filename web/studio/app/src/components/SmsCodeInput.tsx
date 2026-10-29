// 문자 인증번호 6자리 칸 + 남은 시간 + 다시 받기 — 비밀번호 재설정과 휴대폰 번호 등록이 함께 쓴다.
// 모양은 본인인증 창(IdentityVerify)의 인증번호 칸과 같다.
import { useEffect, useState } from 'react';

const secondsUntil = (iso: string) => (iso ? Math.max(0, Math.round((Date.parse(iso) - Date.now()) / 1000)) : 0);

/** 서버가 준 만료 시각까지 남은 초 (1초마다 갱신) */
export function useCountdown(expiresAt: string): number {
  const [left, setLeft] = useState(() => secondsUntil(expiresAt));
  useEffect(() => {
    setLeft(secondsUntil(expiresAt));
    const t = window.setInterval(() => setLeft(secondsUntil(expiresAt)), 1000);
    return () => window.clearInterval(t);
  }, [expiresAt]);
  return left;
}

export function SmsCodeInput({ id, value, onChange, left, onResend, resending }: {
  id: string; value: string; onChange: (code: string) => void;
  left: number; onResend: () => void; resending?: boolean;
}) {
  const [focus, setFocus] = useState(false);
  const mm = String(Math.floor(left / 60));
  const ss = String(left % 60).padStart(2, '0');
  return (
    <>
      <div className="ui-field">
        <label htmlFor={id} className="sr-only">인증번호 6자리</label>
        {/* 입력은 칸 하나로 받고(문자 자동 채우기·붙여넣기 유지), 화면에는 숫자를 한 칸씩 나눠 보여 준다 */}
        <div className="aq-otp">
          <input
            id={id} autoFocus inputMode="numeric" autoComplete="one-time-code" maxLength={6}
            className="aq-otp-input" value={value}
            onChange={e => onChange(e.target.value.replace(/\D/g, '').slice(0, 6))}
            onFocus={() => setFocus(true)} onBlur={() => setFocus(false)}
          />
          {Array.from({ length: 6 }, (_, i) => (
            <span
              key={i} aria-hidden="true"
              className={`aq-otp-cell${value[i] ? ' is-filled' : ''}${focus && i === Math.min(value.length, 5) ? ' is-active' : ''}`}
            >
              {value[i] ?? ''}
            </span>
          ))}
        </div>
      </div>
      <div className="aq-otp-meta">
        <span className={left < 30 ? 'is-low' : ''}>{left > 0 ? `남은 시간 ${mm}:${ss}` : '시간이 지났어요'}</span>
        <button type="button" className="aq-verify-resend" onClick={onResend} disabled={resending}>
          {resending ? '보내는 중' : '인증번호 다시 받기'}
        </button>
      </div>
    </>
  );
}
