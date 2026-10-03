import { loginRedirect } from './lib/loginRedirect';
// 로그인 — 스튜디오 로그인 화면(AuthLayout)과 같은 카드·입력창·버튼. 계정도 스튜디오와 같다.
import { useState } from 'react';
import { useLocation, useNavigate } from './lib/router';
import { useAuth } from './auth';
import { errorMessage } from './api/errors';
import { MOCK } from './lib/mode';

function PasswordInput(props: React.InputHTMLAttributes<HTMLInputElement> & { show: boolean; onToggle: () => void }) {
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

export function Login() {
  const { login } = useAuth();
  const nav = useNavigate();
  const loc = useLocation();
  const from = loginRedirect(loc.state);
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [showPw, setShowPw] = useState(false);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (busy) return;
    setError('');
    setBusy(true);
    try {
      await login(email, password);
      nav(from ?? '/', { replace: true });
    } catch (err) {
      setError(errorMessage(err, '로그인에 실패했어요.'));
      setBusy(false);
    }
  }

  return (
    <div className="auth-page">
      <div className="aq-auth-glow" aria-hidden="true" />
      <div className="auth-split">
        <div className="auth-card">
          <span className="aq-auth-brand" aria-label="AUDENIQ ADMIN">
            <img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" />
            <span>ADMIN</span>
          </span>
          <h1>관리자 콘솔</h1>
          <p className="auth-sub">
            {MOCK ? '체험 모드예요. 아무 이메일로나 들어가 예시 데이터를 볼 수 있어요.' : '스튜디오와 같은 계정으로 로그인해요.'}
          </p>
          {error && <div className="feedback feedback-error aq-shake" role="alert" key={error}>{error}</div>}
          <form onSubmit={submit}>
            <div className="field">
              <label htmlFor="loginEmail">이메일</label>
              <input
                id="loginEmail" type="email" required autoComplete="email" autoFocus
                placeholder="이메일을 입력해 주세요"
                value={email} onChange={e => setEmail(e.target.value)}
              />
            </div>
            <div className="field">
              <label htmlFor="loginPassword">비밀번호</label>
              <PasswordInput
                id="loginPassword" required={!MOCK} autoComplete="current-password"
                placeholder="비밀번호를 입력해 주세요"
                value={password} onChange={e => setPassword(e.target.value)}
                show={showPw} onToggle={() => setShowPw(v => !v)}
              />
            </div>
            <button className={`button auth-submit${busy ? ' is-busy' : ''}`} type="submit" disabled={busy} aria-busy={busy}>
              {busy ? '들어가는 중' : '로그인'}
            </button>
          </form>
        </div>
      </div>
      <p className="auth-foot">
        <span>© AUDENIQ</span>
        <span aria-hidden="true">·</span>
        <span>관리자 권한은 운영 책임자가 부여해요</span>
      </p>
    </div>
  );
}
