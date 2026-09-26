import { useState } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { useAuth } from '../api/auth';

export function Login() {
  const { login } = useAuth();
  const nav = useNavigate();
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setError('');
    setBusy(true);
    try {
      await login(email.trim(), password);
      nav('/');
    } catch (err) {
      setError(err instanceof Error ? err.message : '로그인에 실패했어요.');
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="auth-page">
      <div className="auth-card">
        <img src="/connected/assets/AUDENIQ_Logo_Light.svg" alt="AUDENIQ STUDIO" className="auth-logo" />
        <h1>다시 만나서 반가워요.</h1>
        <p className="auth-sub">아티스트 포털에 로그인하세요.</p>
        {error && <div className="feedback feedback-error" role="alert">{error}</div>}
        <form onSubmit={submit}>
          <div className="field">
            <label htmlFor="loginEmail">이메일</label>
            <input
              id="loginEmail" type="email" required autoComplete="email"
              placeholder="이메일을 입력해 주세요"
              value={email} onChange={e => setEmail(e.target.value)}
            />
          </div>
          <div className="field">
            <label htmlFor="loginPassword">비밀번호</label>
            <input
              id="loginPassword" type="password" required autoComplete="current-password"
              placeholder="비밀번호를 입력해 주세요"
              value={password} onChange={e => setPassword(e.target.value)}
            />
          </div>
          <button className="button auth-submit" type="submit" disabled={busy}>
            {busy ? '들어가는 중...' : '로그인'}
          </button>
        </form>
        <div className="auth-links">
          <Link to="/find-account">아이디 · 비밀번호 찾기</Link>
          <span aria-hidden="true">·</span>
          <Link to="/signup">회원가입</Link>
        </div>
      </div>
    </div>
  );
}
