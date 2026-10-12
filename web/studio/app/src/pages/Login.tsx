import { loginRedirect } from '../lib/loginRedirect';
import { useState } from 'react';
import { Link, useLocation, useNavigate } from '../lib/router';
import { useAuth } from '../api/auth';
import { authErrorMessage } from '../api/errors';
import { AuthLayout, PasswordInput } from '../components/AuthLayout';
import { passkeySupported } from '../api/passkey';
import { MOCK } from '../api/client';
import { ApiError } from '../api/errors';

export function Login() {
  const { login, loginPasskey } = useAuth();
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
      await login(email.trim(), password);
      nav(from ?? '/', { replace: true });
    } catch (err) {
      setError(authErrorMessage(err, 'login'));
      setBusy(false);
    }
  }

  // 이메일을 입력한 뒤 비밀번호 대신 패스키로
  async function withPasskey() {
    if (busy) return;
    setError('');
    setBusy(true);
    try {
      await loginPasskey(email.trim());
      nav(from ?? '/', { replace: true });
    } catch (err) {
      setError(err instanceof ApiError && err.code.startsWith('PASSKEY') ? err.message : authErrorMessage(err, 'login'));
      setBusy(false);
    }
  }

  return (
    <AuthLayout title="다시 만나서 반가워요." sub="아티스트 포털에 로그인하세요.">
      {error && <div className="ui-callout is-danger aq-shake" role="alert" key={error}>{error}</div>}
      <form onSubmit={submit} noValidate={false}>
        <div className="ui-field">
          <label htmlFor="loginEmail">이메일</label>
          <input
            id="loginEmail" type="email" required autoComplete="email" autoFocus
            placeholder="이메일을 입력해 주세요"
            value={email} onChange={e => setEmail(e.target.value)}
          />
        </div>
        <div className="ui-field">
          <label htmlFor="loginPassword">비밀번호</label>
          <PasswordInput
            id="loginPassword" required autoComplete="current-password"
            placeholder="비밀번호를 입력해 주세요"
            value={password} onChange={e => setPassword(e.target.value)}
            show={showPw} onToggle={() => setShowPw(v => !v)}
          />
        </div>
        <button className={`ui-btn is-primary auth-submit${busy ? ' is-busy' : ''}`} type="submit" disabled={busy} aria-busy={busy}>
          {busy ? '들어가는 중' : '로그인'}
        </button>
        {!MOCK && passkeySupported() && (
          <button type="button" className="ui-btn auth-submit aq-passkey-btn" onClick={() => void withPasskey()} disabled={busy}>
            비밀번호 대신 패스키로 로그인
          </button>
        )}
      </form>
      <div className="auth-links">
        <Link to="/find-account">아이디 · 비밀번호 찾기</Link>
        <span aria-hidden="true">·</span>
        <Link to="/signup">회원가입</Link>
      </div>
    </AuthLayout>
  );
}
