import { useEffect, useState } from 'react';
import { Link, useNavigate } from '../lib/router';
import { useAuth } from '../api/auth';
import { PASSWORD_MIN } from '../api/client';
import { authErrorMessage, messageForCode } from '../api/errors';
import { SocialSignIn } from '../components/SocialSignIn';
import { takeOAuthResult } from '../api/oauth';
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
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  // 로그인 화면에서 Google·Apple로 처음 온 경우: 약관에 동의한 뒤 다시 누르도록 안내
  useEffect(() => { const r = takeOAuthResult(); if (r.error) setError(messageForCode(r.error)); }, []);
  const pw = strength(password);
  const mismatch = !!confirm && password !== confirm;

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (busy) return;
    setError('');
    if (!name.trim()) { setError('이름을 입력해 주세요.'); return; }
    if (password.length < PASSWORD_MIN) { setError(`비밀번호는 ${PASSWORD_MIN}자 이상이어야 해요.`); return; }
    if (password !== confirm) { setError('비밀번호가 일치하지 않아요.'); return; }
    if (!agreeTerms) { setError('필수 약관에 동의해 주세요.'); return; }
    setBusy(true);
    try {
      await signup(email.trim(), password);
      patchProfile({ name: name.trim(), email: email.trim() });
      nav('/', { replace: true });
    } catch (err) {
      setError(authErrorMessage(err, 'signup'));
      setBusy(false);
    }
  }

  return (
    <AuthLayout title="AUDENIQ과 함께 시작하세요." sub="아티스트 계정을 만들고 발매를 준비해 보세요.">
      {error && <div className="ui-callout is-danger aq-shake" role="alert" key={error}>{error}</div>}
      <form onSubmit={submit}>
        <div className="ui-field">
          <label htmlFor="suName">이름 (활동명)</label>
          <input
            id="suName" type="text" required autoComplete="name" autoFocus maxLength={120}
            placeholder="이름을 입력해 주세요"
            value={name} onChange={e => setName(e.target.value)}
          />
        </div>
        <div className="ui-field">
          <label htmlFor="suEmail">이메일</label>
          <input
            id="suEmail" type="email" required autoComplete="email"
            placeholder="이메일을 입력해 주세요"
            value={email} onChange={e => setEmail(e.target.value)}
          />
        </div>
        <div className="ui-field">
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
        <div className="ui-field">
          <label htmlFor="suConfirm">비밀번호 확인</label>
          <input
            id="suConfirm" type={showPw ? 'text' : 'password'} required autoComplete="new-password"
            placeholder="비밀번호를 다시 입력해 주세요" aria-invalid={mismatch}
            value={confirm} onChange={e => setConfirm(e.target.value)}
          />
          {mismatch && <p className="ui-field-help aq-help-error">비밀번호가 일치하지 않아요.</p>}
        </div>
        <div className="auth-agree">
          <label className="check-row">
            <input type="checkbox" checked={agreeTerms} onChange={e => setAgreeTerms(e.target.checked)} />
            <span><a href="/terms" target="_blank" rel="noopener" className="aq-inline-link">AUDENIQ 음원 배급 서비스 이용약관</a>에 동의합니다. (필수)</span>
          </label>
          <p className="small muted">
            회원가입 정보(이메일·비밀번호·활동명)는 요청한 계정 생성과 서비스 제공을 위해 처리해요.
            계정정보는 원칙적으로 탈퇴 또는 처리 목적 종료 시까지 보관하며, 법정 보존자료는 별도로 관리해요.
            자세한 항목·보유기간과 권리 행사방법은 <a href="/privacy" target="_blank" rel="noopener" className="aq-inline-link">개인정보 처리방침</a>에서 확인해 주세요.
            선택 기능에 필요한 별도 동의는 해당 기능을 이용할 때 안내해요.
          </p>
        </div>
        <button className={`ui-btn is-primary auth-submit${busy ? ' is-busy' : ''}`} type="submit" disabled={busy} aria-busy={busy}>
          {busy ? '가입하는 중' : '회원가입'}
        </button>
      </form>
      <SocialSignIn
        signup onError={setError}
        canSignup={() => {
          if (agreeTerms) return true;
          setError('이용약관에 동의한 뒤 Google·Apple로 계속할 수 있어요.');
          return false;
        }}
      />
      <div className="auth-links">
        <span>이미 계정이 있으신가요?</span>
        <Link to="/login">로그인</Link>
      </div>
    </AuthLayout>
  );
}

