import { useState } from 'react';
import { Link, useLocation, useNavigate } from 'react-router';
import { useAuth } from '../api/auth';
import { errorMessage } from '../api/errors';
import { AuthLayout, PasswordInput } from '../components/AuthLayout';

export function Login() {
  const { login } = useAuth();
  const nav = useNavigate();
  const loc = useLocation();
  const from = (loc.state as { from?: string } | null)?.from;
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
      nav(from && from !== '/login' ? from : '/', { replace: true });
    } catch (err) {
      setError(errorMessage(err, '로그인에 실패했어요.'));
      setBusy(false);
    }
  }

  return (
    <AuthLayout title="다시 만나서 반가워요." sub="아티스트 포털에 로그인하세요.">
      {error && <div className="feedback feedback-error aq-shake" role="alert" key={error}>{error}</div>}
      <form onSubmit={submit} noValidate={false}>
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
            id="loginPassword" required autoComplete="current-password"
            placeholder="비밀번호를 입력해 주세요"
            value={password} onChange={e => setPassword(e.target.value)}
            show={showPw} onToggle={() => setShowPw(v => !v)}
          />
        </div>
        <button className={`button auth-submit${busy ? ' is-busy' : ''}`} type="submit" disabled={busy} aria-busy={busy}>
          {busy ? '들어가는 중' : '로그인'}
        </button>
      </form>
      <div className="auth-links">
        <Link to="/find-account">아이디 · 비밀번호 찾기</Link>
        <span aria-hidden="true">·</span>
        <Link to="/signup">회원가입</Link>
      </div>
    </AuthLayout>
  );
}
