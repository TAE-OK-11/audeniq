import { loginRedirect } from '../lib/loginRedirect';
import { useEffect, useState } from 'react';
import { Link, useLocation, useNavigate } from '../lib/router';
import { useAuth } from '../api/auth';
import { authErrorMessage } from '../api/errors';
import { AUTO_FOCUS, AuthLayout, EMAIL_INPUT, EmailSuggest, FieldError, PasswordInput, focusFirst, isEmail, nextOnEnter } from '../components/AuthLayout';
import { passkeySupported } from '../api/passkey';
import { MOCK } from '../api/client';
import { ApiError, messageForCode } from '../api/errors';
import { SocialSignIn } from '../components/SocialSignIn';
import { takeOAuthResult } from '../api/oauth';

export function Login() {
  const { login, loginPasskey } = useAuth();
  const nav = useNavigate();
  const loc = useLocation();
  const from = loginRedirect(loc.state);
  // 가입 화면에서 '이미 가입된 이메일 → 이 이메일로 로그인'으로 오면 주소를 채워 둔다
  const presetEmail = (loc.state as { email?: unknown } | null)?.email;
  const [email, setEmail] = useState(typeof presetEmail === 'string' ? presetEmail : '');
  const [emailDone, setEmailDone] = useState(false);
  const [password, setPassword] = useState('');
  const [showPw, setShowPw] = useState(false);
  const [error, setError] = useState('');
  // 이메일·비밀번호가 맞지 않을 때는 오류 안에 '비밀번호 찾기'를 — 아래 작은 링크까지 눈이 가지 않았다
  const [wrongPw, setWrongPw] = useState(false);
  const [fieldErr, setFieldErr] = useState<{ email?: string; password?: string }>({});
  const [busy, setBusy] = useState(false);
  // Google·Apple에서 돌아왔는데 로그인하지 못한 경우
  useEffect(() => { const r = takeOAuthResult(); if (r.error) setError(messageForCode(r.error)); }, []);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (busy) return;
    setError('');
    setWrongPw(false);
    // 브라우저 기본 경고(영어 말풍선) 대신 칸 아래 한국어 안내
    const fe = {
      email: !email.trim() ? '이메일을 입력해 주세요.' : !isEmail(email) ? '이메일 주소를 확인해 주세요.' : undefined,
      password: !password ? '비밀번호를 입력해 주세요.' : undefined,
    };
    setFieldErr(fe);
    if (fe.email || fe.password) { focusFirst([fe.email && 'loginEmail', fe.password && 'loginPassword']); return; }
    setBusy(true);
    try {
      await login(email.trim(), password);
      nav(from ?? '/', { replace: true });
    } catch (err) {
      setError(authErrorMessage(err, 'login'));
      setWrongPw(err instanceof ApiError && (err.code === 'UNAUTHENTICATED' || err.code === 'INVALID_INPUT'));
      setBusy(false);
    }
  }

  // 이메일을 입력한 뒤 비밀번호 대신 패스키로
  async function withPasskey() {
    if (busy) return;
    setError('');
    if (email.trim() && !isEmail(email)) { setFieldErr({ email: '이메일 주소를 확인해 주세요.' }); focusFirst(['loginEmail']); return; }
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
    <AuthLayout icon="lock" title="다시 만나서 반가워요." sub="아티스트 포털에 로그인해 주세요.">
      {error && (
        <div className="ui-callout is-danger aq-shake" role="alert" key={error}>
          {error}
          {wrongPw && <> <Link to="/find-account" className="aq-inline-link">비밀번호 찾기</Link></>}
        </div>
      )}
      <form onSubmit={submit} noValidate>
        <div className="ui-field">
          <label htmlFor="loginEmail">이메일</label>
          <input
            id="loginEmail" {...EMAIL_INPUT} required autoComplete="username" autoFocus={AUTO_FOCUS && !email}
            placeholder="name@example.com" aria-invalid={!!fieldErr.email} aria-describedby={fieldErr.email ? 'loginEmailErr' : undefined}
            value={email} onChange={e => { setEmail(e.target.value); setEmailDone(false); if (fieldErr.email) setFieldErr(f => ({ ...f, email: undefined })); }}
            onBlur={() => setEmailDone(true)} onKeyDown={nextOnEnter('loginPassword')}
          />
          <FieldError id="loginEmailErr">{fieldErr.email}</FieldError>
          {emailDone && !fieldErr.email && <EmailSuggest value={email} onApply={setEmail} />}
        </div>
        <div className="ui-field">
          <label htmlFor="loginPassword">비밀번호</label>
          <PasswordInput
            id="loginPassword" required autoComplete="current-password" autoFocus={AUTO_FOCUS && !!email}
            enterKeyHint="go"
            aria-invalid={!!fieldErr.password} aria-describedby={fieldErr.password ? 'loginPasswordErr' : undefined}
            value={password} onChange={e => { setPassword(e.target.value); if (fieldErr.password) setFieldErr(f => ({ ...f, password: undefined })); }}
            show={showPw} onToggle={() => setShowPw(v => !v)}
          />
          <FieldError id="loginPasswordErr">{fieldErr.password}</FieldError>
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
      <SocialSignIn onError={setError} returnTo={from ?? undefined} />
      <div className="auth-links">
        <Link to="/find-account">이메일 · 비밀번호 찾기</Link>
        <span aria-hidden="true">·</span>
        <Link to="/signup">회원가입</Link>
      </div>
    </AuthLayout>
  );
}
