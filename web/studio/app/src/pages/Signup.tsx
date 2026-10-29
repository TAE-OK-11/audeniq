import { useEffect, useState } from 'react';
import { Link, useNavigate } from '../lib/router';
import { useAuth } from '../api/auth';
import { MOCK, PASSWORD_MIN } from '../api/client';
import { passkeySupported } from '../api/passkey';
import { authErrorMessage, messageForCode } from '../api/errors';
import { SocialSignIn } from '../components/SocialSignIn';
import { takeOAuthResult } from '../api/oauth';
import { AUTO_FOCUS, AuthLayout, EMAIL_INPUT, EmailSuggest, FieldError, PasswordInput, focusFirst, isEmail, nextOnEnter } from '../components/AuthLayout';
import { ApiError } from '../api/errors';
import { CheckIcon } from '../components/Check';
import { patchProfile } from '../store/profile';
import { clearPasskeyHandoff, hasPasskeyHandoff, preparePasskeyHandoff } from '../lib/passkeyHandoff';

export function strength(pw: string): { score: number; label: string } {
  // 최소 길이보다 짧으면 조합과 상관없이 '너무 짧아요' — 6자 'abc123'이 '보통'으로 보였다
  if (pw.length < PASSWORD_MIN) return { score: 0, label: '너무 짧아요' };
  let score = 0;
  if (pw.length >= PASSWORD_MIN) score++;
  if (pw.length >= PASSWORD_MIN + 4) score++;
  if (/[A-Za-z]/.test(pw) && /\d/.test(pw)) score++;
  if (/[^A-Za-z0-9]/.test(pw)) score++;
  const label = ['너무 짧아요', '보통', '괜찮아요', '안전해요', '매우 안전해요'][score];
  return { score, label };
}

/** 휴대폰 번호를 입력하는 대로 010-1234-5678 꼴로 — 숫자만 남겨 11자리까지 */
export function formatPhone(v: string): string {
  const d = v.replace(/\D/g, '').slice(0, 11);
  if (d.length < 4) return d;
  if (d.length < 8) return `${d.slice(0, 3)}-${d.slice(3)}`;
  return `${d.slice(0, 3)}-${d.slice(3, d.length - 4)}-${d.slice(-4)}`;
}
/** 010·011·016~019로 시작하는 10~11자리 휴대폰 번호 */
export const isPhone = (v: string) => /^01[016789]\d{7,8}$/.test(v.replace(/\D/g, ''));

export function Signup() {
  const { signup } = useAuth();
  const nav = useNavigate();
  // 한 번에 다 받지 않고 단계로 — ① 로그인할 이메일·비밀번호 ② 활동명·약관 동의(여기서 계정을 만든다) ③ 패스키(PasskeySetup)
  const [step, setStep] = useState<1 | 2>(1);
  const [back, setBack] = useState(false);
  const steps = !MOCK && passkeySupported() ? 3 : 2;
  const [name, setName] = useState('');
  // 전화번호 — 지금은 입력 칸만 있고 서버로 보내지 않는다(저장할 곳이 아직 없다). 저장을 붙일 때 개인정보 안내 항목도 함께 바꾼다
  const [phone, setPhone] = useState('');
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [confirm, setConfirm] = useState('');
  const [emailDone, setEmailDone] = useState(false);
  const [confirmDone, setConfirmDone] = useState(false);
  // 이미 가입된 이메일 — 오류 안에 '이 이메일로 로그인'을 둔다
  const [taken, setTaken] = useState(false);
  const [showPw, setShowPw] = useState(false);
  const [agreeTerms, setAgreeTerms] = useState(false);
  // 1단계에서 Google·Apple을 눌렀는데 약관에 아직 동의하지 않았으면 그 자리에 동의 칸을 보여 준다
  const [socialTerms, setSocialTerms] = useState(false);
  const [error, setError] = useState('');
  type Fe = { name?: string; phone?: string; email?: string; password?: string; confirm?: string; terms?: string };
  const [fe, setFe] = useState<Fe>({});
  const clear = (k: keyof Fe) => { if (fe[k]) setFe(f => ({ ...f, [k]: undefined })); };
  const [busy, setBusy] = useState(false);
  // 로그인 화면에서 Google·Apple로 처음 온 경우: 약관에 동의한 뒤 다시 누르도록 안내
  useEffect(() => { const r = takeOAuthResult(); if (r.error) setError(messageForCode(r.error)); }, []);
  const pw = strength(password);
  // 확인 칸은 한 글자만 쳐도 '일치하지 않아요'가 떴다 — 다 입력했거나(길이가 같아짐) 칸을 떠났을 때만 판단한다
  const confirmReady = !!confirm && (confirmDone || confirm.length >= password.length);
  const mismatch = confirmReady && password !== confirm;
  const matched = confirmReady && !!password && password === confirm;
  const longEnough = password.length >= PASSWORD_MIN;

  const go = (to: 1 | 2) => {
    setBack(to < step);
    setStep(to);
    setError('');
    // 다음 단계 첫 칸으로 — 휴대폰 키보드가 내려갔다 올라오지 않게 바로 옮긴다
    requestAnimationFrame(() => document.getElementById(to === 1 ? 'suEmail' : 'suName')?.focus({ preventScroll: true }));
  };

  // 1단계 — 이메일·비밀번호만 확인하고 다음으로 (이미 가입된 이메일인지는 계정을 만들 때 서버가 알려 준다)
  function next(e: React.FormEvent) {
    e.preventDefault();
    setError('');
    setTaken(false);
    const n: Fe = {
      email: !email.trim() ? '이메일을 입력해 주세요.' : !isEmail(email) ? '이메일 주소를 확인해 주세요.' : undefined,
      password: password.length < PASSWORD_MIN ? `비밀번호는 ${PASSWORD_MIN}자 이상이어야 해요.` : undefined,
      confirm: !confirm ? '비밀번호를 한 번 더 입력해 주세요.' : password !== confirm ? '비밀번호가 일치하지 않아요.' : undefined,
    };
    setFe(n);
    if (n.email || n.password || n.confirm) { focusFirst([n.email && 'suEmail', n.password && 'suPassword', n.confirm && 'suConfirm']); return; }
    go(2);
  }

  // 2단계 — 활동명·약관을 받고 계정을 만든다. 그다음 패스키 단계(PasskeySetup)로
  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (busy) return;
    setError('');
    setTaken(false);
    const n: Fe = {
      name: !name.trim() ? '활동명을 입력해 주세요.' : undefined,
      phone: !phone ? '전화번호를 입력해 주세요.' : !isPhone(phone) ? '휴대폰 번호를 확인해 주세요.' : undefined,
      terms: !agreeTerms ? '이용약관에 동의해 주세요.' : undefined,
    };
    setFe(n);
    if (n.name || n.phone || n.terms) { focusFirst([n.name && 'suName', n.phone && 'suPhone', n.terms && 'suTerms']); return; }
    setBusy(true);
    // 가입 직후 이 기기에 패스키를 만들지 묻는다 — 다음 로그인부터 비밀번호 없이 (pages/PasskeySetup)
    preparePasskeyHandoff(password, '/', true);
    try {
      await signup(email.trim(), password);
      patchProfile({ name: name.trim(), email: email.trim() });
      nav(hasPasskeyHandoff() ? '/passkey-setup' : '/', { replace: true });
    } catch (err) {
      clearPasskeyHandoff();
      setBusy(false);
      // 이미 가입된 이메일이면 이메일 단계로 돌아가 그 칸에서 알려 준다
      if (err instanceof ApiError && err.code === 'CONFLICT') {
        go(1);
        setTaken(true);
        setError(authErrorMessage(err, 'signup'));
        return;
      }
      setError(authErrorMessage(err, 'signup'));
    }
  }

  const terms = (
    <div className="auth-agree">
      <label className="ui-check auth-terms">
        <input id="suTerms" type="checkbox" checked={agreeTerms} aria-invalid={!!fe.terms} aria-describedby={fe.terms ? 'suTermsErr' : undefined}
          onChange={e => { setAgreeTerms(e.target.checked); clear('terms'); }} />
        <span><a href="/terms" target="_blank" rel="noopener" className="aq-inline-link">AUDENIQ 이용약관</a>에 동의해요. <em className="auth-req">필수</em></span>
      </label>
      <FieldError id="suTermsErr">{fe.terms}</FieldError>
    </div>
  );

  return (
    <AuthLayout
      icon={step === 1 ? 'user' : 'music'} step={{ at: step, of: steps }}
      title={step === 1 ? 'AUDENIQ과 함께 시작해요.' : '어떤 이름으로 활동하나요?'}
      sub={step === 1 ? '먼저 로그인에 쓸 이메일과 비밀번호를 정해 주세요.' : ''}
    >
      {error && (
        <div className="ui-callout is-danger aq-shake" role="alert" key={error}>
          {error}
          {taken && <> <Link to="/login" state={{ email: email.trim() }} className="aq-inline-link">이 이메일로 로그인</Link></>}
        </div>
      )}
      {step === 1 ? (
        <form key="s1" onSubmit={next} noValidate className={`aq-step-pane${back ? ' is-back' : ''}`}>
          <div className="ui-field">
            <label htmlFor="suEmail">이메일</label>
            <input
              id="suEmail" {...EMAIL_INPUT} required autoComplete="email" autoFocus={AUTO_FOCUS}
              placeholder="name@example.com" aria-invalid={!!fe.email} aria-describedby={fe.email ? 'suEmailErr' : undefined}
              value={email} onChange={e => { setEmail(e.target.value); setEmailDone(false); setTaken(false); clear('email'); }}
              onBlur={() => setEmailDone(true)} onKeyDown={nextOnEnter('suPassword')}
            />
            <FieldError id="suEmailErr">{fe.email}</FieldError>
            {emailDone && !fe.email && <EmailSuggest value={email} onApply={setEmail} />}
          </div>
          <div className="ui-field">
            {/* 조건은 이름표 옆에 늘 보이게 — 칸 안 안내 글자는 치기 시작하면 사라졌다. 채우면 초록 체크 */}
            <label htmlFor="suPassword">
              비밀번호
              {/* 길이를 채우면 같은 자리에 강도를 — 칸 아래 강도 막대가 생기며 카드가 늘어나지 않게 */}
              <span className={`aq-pw-rule${longEnough ? ' is-ok' : ''}`} id="suPwRule" data-score={longEnough ? pw.score : undefined} aria-live="polite">
                {longEnough ? <><CheckIcon size={12} />{pw.label}</> : `${PASSWORD_MIN}자 이상`}
              </span>
            </label>
            <PasswordInput
              id="suPassword" required autoComplete="new-password" minLength={PASSWORD_MIN}
              aria-describedby={fe.password ? 'suPasswordErr' : 'suPwRule'}
              aria-invalid={!!fe.password} enterKeyHint="next"
              value={password} onChange={e => { setPassword(e.target.value); clear('password'); }} onKeyDown={nextOnEnter('suConfirm')}
              show={showPw} onToggle={() => setShowPw(v => !v)}
            />
            <FieldError id="suPasswordErr">{fe.password}</FieldError>
          </div>
          <div className="ui-field">
            <label htmlFor="suConfirm">비밀번호 확인</label>
            <input
              id="suConfirm" type={showPw ? 'text' : 'password'} required autoComplete="new-password"
              placeholder="한 번 더 입력해 주세요" aria-invalid={mismatch || !!fe.confirm} enterKeyHint="next"
              aria-describedby={fe.confirm || mismatch ? 'suConfirmErr' : matched ? 'suConfirmOk' : undefined}
              value={confirm} onChange={e => { setConfirm(e.target.value); setConfirmDone(false); clear('confirm'); }}
              onBlur={() => setConfirmDone(true)}
            />
            <FieldError id="suConfirmErr">{fe.confirm ?? (mismatch ? '비밀번호가 일치하지 않아요.' : undefined)}</FieldError>
            {matched && !fe.confirm && <p className="ui-field-help aq-pw-rule is-ok" id="suConfirmOk"><CheckIcon size={12} />비밀번호가 일치해요</p>}
          </div>
          <button className="ui-btn is-primary auth-submit" type="submit">다음</button>
          {socialTerms && terms}
          <SocialSignIn
            signup onError={setError}
            canSignup={() => {
              if (agreeTerms) return true;
              // Google·Apple로 바로 가입할 때는 이 자리에서 약관 동의를 받는다
              setSocialTerms(true);
              setFe(f => ({ ...f, terms: '이용약관에 동의한 뒤 Google·Apple로 계속할 수 있어요.' }));
              requestAnimationFrame(() => focusFirst(['suTerms']));
              return false;
            }}
          />
        </form>
      ) : (
        <form key="s2" onSubmit={submit} noValidate className={`aq-step-pane${back ? ' is-back' : ''}`}>
          <div className="ui-field">
            {/* 활동명은 실명이 아니라 플랫폼에 보일 이름 — 자동 완성이 실명을 채우지 않게 nickname */}
            <label htmlFor="suName">활동명</label>
            <input
              id="suName" type="text" required autoComplete="nickname" maxLength={120} enterKeyHint="next"
              placeholder="음원 플랫폼에 보일 이름 · 나중에 바꿀 수 있어요" aria-invalid={!!fe.name} aria-describedby={fe.name ? 'suNameErr' : undefined}
              value={name} onChange={e => { setName(e.target.value); clear('name'); }} onKeyDown={nextOnEnter('suPhone')}
            />
            <FieldError id="suNameErr">{fe.name}</FieldError>
          </div>
          <div className="ui-field">
            <label htmlFor="suPhone">휴대폰 번호</label>
            <input
              id="suPhone" type="tel" inputMode="numeric" required autoComplete="tel-national" enterKeyHint="done" maxLength={13}
              placeholder="010-1234-5678" aria-invalid={!!fe.phone} aria-describedby={fe.phone ? 'suPhoneErr' : undefined}
              value={phone} onChange={e => { setPhone(formatPhone(e.target.value)); clear('phone'); }}
            />
            <FieldError id="suPhoneErr">{fe.phone}</FieldError>
          </div>
          {terms}
          {/* 개인정보 안내는 첫 줄만 보이고 눌러서 펼친다 — 다섯 줄이 카드를 길게 늘였다. 내용은 그대로 */}
          <details className="auth-privacy">
            <summary>가입 정보는 계정 생성과 서비스 제공에 써요. <span>자세히</span></summary>
            <p>
              회원가입 정보(이메일·비밀번호·활동명)는 요청한 계정 생성과 서비스 제공을 위해 처리해요.
              계정정보는 원칙적으로 탈퇴 또는 처리 목적 종료 시까지 보관하며, 법정 보존자료는 별도로 관리해요.
              자세한 항목·보유기간과 권리 행사방법은 <a href="/privacy" target="_blank" rel="noopener" className="aq-inline-link">개인정보 처리방침</a>에서 확인해 주세요.
              선택 기능에 필요한 별도 동의는 해당 기능을 이용할 때 안내해요.
            </p>
          </details>
          <button className={`ui-btn is-primary auth-submit${busy ? ' is-busy' : ''}`} type="submit" disabled={busy} aria-busy={busy}>
            {busy ? '계정을 만드는 중' : steps === 3 ? '계정 만들고 다음' : '가입 완료'}
          </button>
          <button type="button" className="ui-btn is-text auth-later" onClick={() => go(1)} disabled={busy}>이전으로</button>
        </form>
      )}
      {/* 로그인으로 가는 길은 첫 단계에만 — 2단계에는 '이전으로'가 있다 */}
      {step === 1 && (
        <div className="auth-links">
          <span>이미 계정이 있으신가요?</span>
          <Link to="/login">로그인</Link>
        </div>
      )}
    </AuthLayout>
  );
}
