import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
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
      await login(email, password);
      nav('/');
    } catch (err) {
      setError(err instanceof Error ? err.message : '로그인 실패');
    } finally {
      setBusy(false);
    }
  }

  return (
    <div style={{ maxWidth: 400, margin: '80px auto' }}>
      <h1 className="section-title" style={{ textAlign: 'center' }}>AUDENIQ <span style={{ color: 'var(--blue)' }}>STUDIO</span></h1>
      {error && <div className="feedback feedback-error">{error}</div>}
      <div className="card">
        <form onSubmit={submit}>
          <div className="field">
            <label htmlFor="email">이메일</label>
            <input id="email" type="email" value={email} onChange={e => setEmail(e.target.value)} required autoComplete="email" />
          </div>
          <div className="field">
            <label htmlFor="pw">비밀번호</label>
            <input id="pw" type="password" value={password} onChange={e => setPassword(e.target.value)} required autoComplete="current-password" />
          </div>
          <button className="btn" style={{ width: '100%' }} disabled={busy}>
            {busy ? '로그인 중...' : '로그인'}
          </button>
        </form>
      </div>
    </div>
  );
}
