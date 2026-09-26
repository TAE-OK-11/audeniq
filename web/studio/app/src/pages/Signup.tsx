import { useState } from 'react';
import { Link, useNavigate } from '../lib/router';
import { useAuth } from '../api/auth';
import { PASSWORD_MIN } from '../api/client';
import { errorMessage } from '../api/errors';
import { AuthLayout, PasswordInput } from '../components/AuthLayout';
import { patchProfile } from '../store/profile';

function strength(pw: string): { score: number; label: string } {
  let score = 0;
  if (pw.length >= PASSWORD_MIN) score++;
  if (pw.length >= PASSWORD_MIN + 4) score++;
  if (/[A-Za-z]/.test(pw) && /\d/.test(pw)) score++;
  if (/[^A-Za-z0-9]/.test(pw)) score++;
  const label = ['너무 짧아요', '보통', '괜찮아요', '안전해요', '매우 안전해요'][score];
  return { score, label };
}

export function Signup() {
  const { signup } = useAuth();
  const nav = useNavigate();
  const [name, setName] = useState('');
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [confirm, setConfirm] = useState('');
  const [showPw, setShowPw] = useState(false);
  const [agreeTerms, setAgreeTerms] = useState(false);
  const [agreePrivacy, setAgreePrivacy] = useState(false);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const pw = strength(password);
  const mismatch = !!confirm && password !== confirm;

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (busy) return;
    setError('');
    if (!name.trim()) { setError('이름을 입력해 주세요.'); return; }
    if (password.length < PASSWORD_MIN) { setError(`비밀번호는 ${PASSWORD_MIN}자 이상이어야 해요.`); return; }
    if (password !== confirm) { setError('비밀번호가 일치하지 않아요.'); return; }
    if (!agreeTerms || !agreePrivacy) { setError('필수 약관에 동의해 주세요.'); return; }
    setBusy(true);
    try {
      await signup(email.trim(), password);
      patchProfile({ name: name.trim(), email: email.trim() });
      nav('/', { replace: true });
    } catch (err) {
      setError(errorMessage(err, '회원가입에 실패했어요.'));
      setBusy(false);
    }
  }

  return (
    <AuthLayout title="AUDENIQ와 함께 시작하세요." sub="아티스트 계정을 만들고 발매를 준비해 보세요.">
      {error && <div className="feedback feedback-error aq-shake" role="alert" key={error}>{error}</div>}
      <form onSubmit={submit}>
        <div className="field">
          <label htmlFor="suName">이름 (활동명)</label>
          <input
            id="suName" type="text" required autoComplete="name" autoFocus maxLength={120}
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
          <PasswordInput
            id="suPassword" required autoComplete="new-password" minLength={PASSWORD_MIN}
            placeholder={`${PASSWORD_MIN}자 이상 입력해 주세요`} aria-describedby="suPwHelp"
            value={password} onChange={e => setPassword(e.target.value)}
            show={showPw} onToggle={() => setShowPw(v => !v)}
          />
          {password && (
            <div className="aq-pw-meter" data-score={pw.score} id="suPwHelp">
              <span><i /></span>
              <small>{pw.label}</small>
            </div>
          )}
        </div>
        <div className="field">
          <label htmlFor="suConfirm">비밀번호 확인</label>
          <input
            id="suConfirm" type={showPw ? 'text' : 'password'} required autoComplete="new-password"
            placeholder="비밀번호를 다시 입력해 주세요" aria-invalid={mismatch}
            value={confirm} onChange={e => setConfirm(e.target.value)}
          />
          {mismatch && <p className="help aq-help-error">비밀번호가 일치하지 않아요.</p>}
        </div>
        <div className="auth-agree">
          <label className="check-row">
            <input
              type="checkbox" checked={agreeTerms && agreePrivacy}
              onChange={e => { setAgreeTerms(e.target.checked); setAgreePrivacy(e.target.checked); }}
            />
            <span><strong>전체 동의</strong></span>
          </label>
          <label className="check-row">
            <input type="checkbox" checked={agreeTerms} onChange={e => setAgreeTerms(e.target.checked)} />
            <span>이용약관에 동의합니다. (필수)</span>
          </label>
          <label className="check-row">
            <input type="checkbox" checked={agreePrivacy} onChange={e => setAgreePrivacy(e.target.checked)} />
            <span>개인정보 처리방침에 동의합니다. (필수)</span>
          </label>
        </div>
        <button className={`button auth-submit${busy ? ' is-busy' : ''}`} type="submit" disabled={busy} aria-busy={busy}>
          {busy ? '가입하는 중' : '회원가입'}
        </button>
      </form>
      <div className="auth-links">
        <span>이미 계정이 있으신가요?</span>
        <Link to="/login">로그인</Link>
      </div>
    </AuthLayout>
  );
}
