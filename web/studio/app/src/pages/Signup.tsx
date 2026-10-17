import { useEffect, useState } from 'react';
import { Link, useNavigate } from '../lib/router';
import { useAuth } from '../api/auth';
import { PASSWORD_MIN } from '../api/client';
import { authErrorMessage, messageForCode } from '../api/errors';
import { SocialSignIn } from '../components/SocialSignIn';
import { takeOAuthResult } from '../api/oauth';
import { AUTO_FOCUS, AuthLayout, EMAIL_INPUT, FieldError, PasswordInput, focusFirst, isEmail } from '../components/AuthLayout';
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
  type Fe = { name?: string; email?: string; password?: string; confirm?: string; terms?: string };
  const [fe, setFe] = useState<Fe>({});
  const clear = (k: keyof Fe) => { if (fe[k]) setFe(f => ({ ...f, [k]: undefined })); };
  const [busy, setBusy] = useState(false);
  // 로그인 화면에서 Google·Apple로 처음 온 경우: 약관에 동의한 뒤 다시 누르도록 안내
  useEffect(() => { const r = takeOAuthResult(); if (r.error) setError(messageForCode(r.error)); }, []);
  const pw = strength(password);
  const mismatch = !!confirm && password !== confirm;

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (busy) return;
    setError('');
    // 브라우저 기본 경고(영어 말풍선) 대신 칸마다 한국어 안내를 한 번에 보여 주고 첫 칸으로 옮긴다
    const next: Fe = {
      name: !name.trim() ? '이름(활동명)을 입력해 주세요.' : undefined,
      email: !email.trim() ? '이메일을 입력해 주세요.' : !isEmail(email) ? '이메일 주소를 확인해 주세요.' : undefined,
      password: password.length < PASSWORD_MIN ? `비밀번호는 ${PASSWORD_MIN}자 이상이어야 해요.` : undefined,
      confirm: !confirm ? '비밀번호를 한 번 더 입력해 주세요.' : password !== confirm ? '비밀번호가 일치하지 않아요.' : undefined,
      terms: !agreeTerms ? '이용약관에 동의해 주세요.' : undefined,
    };
    setFe(next);
    if (Object.values(next).some(Boolean)) {
      focusFirst([next.name && 'suName', next.email && 'suEmail', next.password && 'suPassword', next.confirm && 'suConfirm', next.terms && 'suTerms']);
      return;
    }
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
    <AuthLayout icon="user" title="AUDENIQ과 함께 시작해요." sub="아티스트 계정을 만들고 발매를 준비해 보세요.">
      {error && <div className="ui-callout is-danger aq-shake" role="alert" key={error}>{error}</div>}
      <form onSubmit={submit} noValidate>
        <div className="ui-field">
          <label htmlFor="suName">이름 (활동명)</label>
          <input
            id="suName" type="text" required autoComplete="name" autoFocus={AUTO_FOCUS} maxLength={120} enterKeyHint="next"
            placeholder="이름을 입력해 주세요" aria-invalid={!!fe.name} aria-describedby={fe.name ? 'suNameErr' : undefined}
            value={name} onChange={e => { setName(e.target.value); clear('name'); }}
          />
          <FieldError id="suNameErr">{fe.name}</FieldError>
        </div>
        <div className="ui-field">
          <label htmlFor="suEmail">이메일</label>
          <input
            id="suEmail" {...EMAIL_INPUT} required autoComplete="email"
            placeholder="이메일을 입력해 주세요" aria-invalid={!!fe.email} aria-describedby={fe.email ? 'suEmailErr' : undefined}
            value={email} onChange={e => { setEmail(e.target.value); clear('email'); }}
          />
          <FieldError id="suEmailErr">{fe.email}</FieldError>
        </div>
        <div className="ui-field">
          <label htmlFor="suPassword">비밀번호</label>
          <PasswordInput
            id="suPassword" required autoComplete="new-password" minLength={PASSWORD_MIN}
            placeholder={`${PASSWORD_MIN}자 이상 입력해 주세요`} aria-describedby={fe.password ? 'suPasswordErr' : 'suPwHelp'}
            aria-invalid={!!fe.password} enterKeyHint="next"
            value={password} onChange={e => { setPassword(e.target.value); clear('password'); }}
            show={showPw} onToggle={() => setShowPw(v => !v)}
          />
          <FieldError id="suPasswordErr">{fe.password}</FieldError>
          {password && !fe.password && (
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
            placeholder="비밀번호를 다시 입력해 주세요" aria-invalid={mismatch || !!fe.confirm} enterKeyHint="done"
            aria-describedby={fe.confirm || mismatch ? 'suConfirmErr' : undefined}
            value={confirm} onChange={e => { setConfirm(e.target.value); clear('confirm'); }}
          />
          <FieldError id="suConfirmErr">{fe.confirm ?? (mismatch ? '비밀번호가 일치하지 않아요.' : undefined)}</FieldError>
        </div>
        <div className="auth-agree">
          <label className="ui-check auth-terms">
            <input id="suTerms" type="checkbox" checked={agreeTerms} aria-invalid={!!fe.terms} aria-describedby={fe.terms ? 'suTermsErr' : undefined}
              onChange={e => { setAgreeTerms(e.target.checked); clear('terms'); }} />
            <span><a href="/terms" target="_blank" rel="noopener" className="aq-inline-link">AUDENIQ 음원 배급 서비스 이용약관</a>에 동의합니다. <em className="auth-req">필수</em></span>
          </label>
          <FieldError id="suTermsErr">{fe.terms}</FieldError>
          <p className="auth-privacy">
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

