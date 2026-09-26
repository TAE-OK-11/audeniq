import type { ReactNode } from 'react';
import { Link } from 'react-router';

export function AuthLayout({ title, sub, children }: { title: string; sub: string; children: ReactNode }) {
  return (
    <div className="auth-page">
      <div className="aq-auth-glow" aria-hidden="true" />
      <div className="auth-split">
        <div className="auth-card">
          <Link to="/login" className="aq-auth-brand" aria-label="AUDENIQ STUDIO">
            <img src={`${import.meta.env.BASE_URL}assets/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" />
            <span>STUDIO</span>
          </Link>
          <h1>{title}</h1>
          <p className="auth-sub">{sub}</p>
          {children}
        </div>
      </div>
      <p className="auth-foot">
        <span>© AUDENIQ</span>
        <span aria-hidden="true">·</span>
        <Link to="/find-account">도움이 필요하신가요?</Link>
      </p>
    </div>
  );
}

/** 비밀번호 보기/숨기기 입력 */
export function PasswordInput(props: React.InputHTMLAttributes<HTMLInputElement> & { show: boolean; onToggle: () => void }) {
  const { show, onToggle, ...rest } = props;
  return (
    <div className="aq-password">
      <input {...rest} type={show ? 'text' : 'password'} />
      <button type="button" className="aq-password-toggle" onClick={onToggle} aria-label={show ? '비밀번호 숨기기' : '비밀번호 보기'} aria-pressed={show}>
        {show ? '숨기기' : '보기'}
      </button>
    </div>
  );
}
