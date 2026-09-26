import { useState } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { useAuth } from '../api/auth';

export function Signup() {
  const { signup } = useAuth();
  const nav = useNavigate();
  const [name, setName] = useState('');
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [confirm, setConfirm] = useState('');
  const [agreeTerms, setAgreeTerms] = useState(false);
  const [agreePrivacy, setAgreePrivacy] = useState(false);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setError('');
    if (password.length < 8) { setError('비밀번호는 8자 이상이어야 해요.'); return; }
    if (password !== confirm) { setError('비밀번호가 일치하지 않아요.'); return; }
    if (!agreeTerms || !agreePrivacy) { setError('필수 약관에 동의해 주세요.'); return; }
    setBusy(true);
    try {
      await signup(email.trim(), password);
      nav('/');
    } catch (err) {
      setError(err instanceof Error ? err.message : '회원가입에 실패했어요.');
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="auth-page">
      <div className="auth-card">
        <img src="/connected/assets/AUDENIQ_Logo_Light.svg" alt="AUDENIQ STUDIO" className="auth-logo" />
        <h1>AUDENIQ와 함께 시작하세요.</h1>
        <p className="auth-sub">아티스트 계정을 만들고 발매를 준비해 보세요.</p>
        {error && <div className="feedback feedback-error" role="alert">{error}</div>}
        <form onSubmit={submit}>
          <div className="field">
            <label htmlFor="suName">이름</label>
            <input
              id="suName" type="text" required autoComplete="name"
              placeholder="이름을 입력해 주세요"
              value={name} onChange={e => setName(e.target.value)}
            />
          </div>
          <div className="field">
            <label htmlFor="suEmail">이메일</label>
            <input
              id="suEmail" type="email" required autoComplete="email"
              placeholder="이메일을 입력해 주세요"
              value={email} onChange={e => setEmail(e.target.value)}
            />
          </div>
          <div className="field">
            <label htmlFor="suPassword">비밀번호</label>
            <input
              id="suPassword" type="password" required autoComplete="new-password"
              placeholder="8자 이상 입력해 주세요"
              value={password} onChange={e => setPassword(e.target.value)}
            />
          </div>
          <div className="field">
            <label htmlFor="suConfirm">비밀번호 확인</label>
            <input
              id="suConfirm" type="password" required autoComplete="new-password"
              placeholder="비밀번호를 다시 입력해 주세요"
              value={confirm} onChange={e => setConfirm(e.target.value)}
            />
          </div>
          <div className="auth-agree">
            <label className="check-row">
              <input type="checkbox" checked={agreeTerms} onChange={e => setAgreeTerms(e.target.checked)} />
              <span>이용약관에 동의합니다. (필수)</span>
            </label>
            <label className="check-row">
              <input type="checkbox" checked={agreePrivacy} onChange={e => setAgreePrivacy(e.target.checked)} />
              <span>개인정보 처리방침에 동의합니다. (필수)</span>
            </label>
          </div>
          <button className="button auth-submit" type="submit" disabled={busy}>
            {busy ? '가입하는 중...' : '회원가입'}
          </button>
        </form>
        <div className="auth-links">
          <span>이미 계정이 있으신가요?</span>
          <Link to="/login">로그인</Link>
        </div>
      </div>
    </div>
  );
}
