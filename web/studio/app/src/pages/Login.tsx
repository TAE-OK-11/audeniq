import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useAuth } from '../api/auth';

export function Login() {
  const { login } = useAuth();
  const nav = useNavigate();
  const [email, setEmail] = useState('test@audeniq.kr');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setError('');
    setBusy(true);
    try {
      await login(email, '');
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
          <button className="btn" style={{ width: '100%' }} disabled={busy}>
            {busy ? '들어가는 중...' : '로그인 (테스트)'}
          </button>
        </form>
      </div>
    </div>
  );
}
