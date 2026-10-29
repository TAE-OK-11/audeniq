import { Glyph } from '../components/Glyph';
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
import { clearPasskeyHandoff, hasPasskeyHandoff, preparePasskeyHandoff } from '../lib/passkeyHandoff';
import { readJSON, writeJSON } from '../lib/storage';

/** 이 기기에서 마지막으로 로그인한 방법 — 비밀번호로 들어왔던 기기면 이메일 칸을 처음부터 펼쳐 둔다 */
const METHOD_KEY = 'login.lastMethod';

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
  const passkeyFirst = !MOCK && passkeySupported();
  // 패스키가 첫 길이면 이메일·비밀번호 칸은 접어 두고 '이메일로 로그인'을 누를 때 펼친다 — 카드가 짧아 휴대폰 한 화면에 들어온다.
  // 패스키를 못 쓰는 기기, 이메일을 채워서 온 경우, 이 기기에서 지난번에 비밀번호로 들어온 경우는 처음부터 펼친다
  const [showForm, setShowForm] = useState(() => !passkeyFirst || typeof presetEmail === 'string' || readJSON<string>(METHOD_KEY, '') === 'password');
  const openForm = () => {
    setShowForm(true);
    requestAnimationFrame(() => document.getElementById('loginEmail')?.focus());
  };
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
    // 비밀번호로 들어오면 바로 다음 화면에서 이 기기에 패스키를 만들지 묻는다 (pages/PasskeySetup)
    preparePasskeyHandoff(password, from ?? '/');
    try {
      await login(email.trim(), password);
      writeJSON(METHOD_KEY, 'password');
      nav(hasPasskeyHandoff() ? '/passkey-setup' : from ?? '/', { replace: true });
    } catch (err) {
      clearPasskeyHandoff();
      setError(authErrorMessage(err, 'login'));
      setWrongPw(err instanceof ApiError && (err.code === 'UNAUTHENTICATED' || err.code === 'INVALID_INPUT'));
      setBusy(false);
    }
  }

  // 패스키 — 이메일 없이 기기에 저장된 패스키를 고르면 바로 로그인 (이메일을 적었으면 그 계정의 패스키로)
  async function withPasskey() {
    if (busy) return;
    setError('');
    setFieldErr({});
    if (email.trim() && !isEmail(email)) { setFieldErr({ email: '이메일 주소를 확인해 주세요.' }); focusFirst(['loginEmail']); return; }
    setBusy(true);
    try {
      await loginPasskey(email.trim());
      writeJSON(METHOD_KEY, 'passkey');
      nav(from ?? '/', { replace: true });
    } catch (err) {
      setError(err instanceof ApiError && err.code.startsWith('PASSKEY') ? err.message : authErrorMessage(err, 'login'));
      setBusy(false);
    }
  }

  const errorBox = error && (
    <div className="ui-callout is-danger aq-shake" role="alert" key={error}>
      {error}
      {wrongPw && <> <Link to="/find-account" className="aq-inline-link">비밀번호 찾기</Link></>}
    </div>
  );

  // 패스키를 쓸 수 있는 기기면 패스키가 첫 번째 길 — 큰 버튼 하나로 바로, 이메일·비밀번호는 그 아래 '또는'
  return (
    <AuthLayout icon={passkeyFirst ? 'key' : 'lock'} title="다시 만나서 반가워요." sub={passkeyFirst ? '패스키가 있으면 비밀번호 없이 바로 들어가요.' : '아티스트 포털에 로그인해 주세요.'}>
      {errorBox}
      {passkeyFirst && (
        <div className="aq-passkey-first">
          <button type="button" className={`ui-btn is-primary auth-submit aq-passkey-main${busy ? ' is-busy' : ''}`} onClick={() => void withPasskey()} disabled={busy}>
            <Glyph name="key" size={20} />
            <span>패스키로 로그인</span>
          </button>
          <p className="auth-fine">지문·얼굴·화면 잠금(PIN)으로 확인해요.</p>
          {showForm
            ? <div className="aq-social-divider" role="separator"><span>또는 이메일로 로그인</span></div>
            : (
              <button type="button" className="ui-btn is-soft auth-submit aq-login-email-open" onClick={openForm} disabled={busy} aria-expanded={false} aria-controls="loginForm">
                <Glyph name="mail" size={18} />
                <span>이메일로 로그인</span>
              </button>
            )}
        </div>
      )}
      {showForm && <form id="loginForm" onSubmit={submit} noValidate className={passkeyFirst ? 'aq-login-form-open' : undefined}>
        <div className="ui-field">
          <label htmlFor="loginEmail">이메일</label>
          <input
            id="loginEmail" {...EMAIL_INPUT} required autoComplete="username webauthn" autoFocus={AUTO_FOCUS && !email && !passkeyFirst}
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
        {/* 패스키가 첫 길이면 비밀번호 로그인은 옅은 버튼 — 파란 버튼은 한 화면에 하나 */}
        <button className={`ui-btn ${passkeyFirst ? 'is-soft' : 'is-primary'} auth-submit${busy ? ' is-busy' : ''}`} type="submit" disabled={busy} aria-busy={busy}>
          {busy ? '들어가는 중' : passkeyFirst ? '비밀번호로 로그인' : '로그인'}
        </button>
      </form>}
      <SocialSignIn onError={setError} returnTo={from ?? undefined} />
      <div className="auth-links">
        <Link to="/find-account">이메일 · 비밀번호 찾기</Link>
        <span aria-hidden="true">·</span>
        <Link to="/signup">회원가입</Link>
      </div>
    </AuthLayout>
  );
}
