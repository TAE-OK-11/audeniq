import { useEffect, useRef, useState } from 'react';
import { Link, useNavigate, useSearchParams } from '../lib/router';
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
import { rememberLoginEmail } from '../lib/lastEmail';
import { formatPhone, isPhone } from '../lib/phone';

export { formatPhone, isPhone };
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

export function Signup() {
  const { signup } = useAuth();
  const nav = useNavigate();
  // 한 번에 다 받지 않고 단계로 — ① 로그인할 이메일·비밀번호 ② 활동명·약관 동의(여기서 계정을 만든다) ③ 패스키(PasskeySetup)
  // 단계는 주소(?step=2)에도 남긴다 — 휴대폰 '뒤로'를 누르면 가입 화면을 떠나지 않고 앞 단계로, 적은 내용은 그대로
  const [params, setParams] = useSearchParams();
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
  // 1단계를 마쳐야 2단계 — 새로 고침 등으로 비어 있는데 주소만 ?step=2면 1단계를 보여 준다
  const step1Done = isEmail(email) && longEnough && password === confirm;
  const step: 1 | 2 = params.get('step') === '2' && step1Done ? 2 : 1;
  // 앞으로 왔는지 뒤로 왔는지 — 단계 칸이 들어오는 방향(뒤로면 왼쪽에서)
  const [shown, setShown] = useState(step);
  const [back, setBack] = useState(false);
  if (shown !== step) { setBack(step < shown); setShown(step); }
  const prevStep = useRef(step);
  const pushed = useRef(false);
  useEffect(() => {
    if (params.get('step') === '2' && !step1Done) setParams({}, { replace: true });
  }, [params, step1Done, setParams]);
  // 단계가 바뀌면 그 단계 첫 칸으로 — 휴대폰 키보드가 내려갔다 올라오지 않게 바로 옮긴다
  useEffect(() => {
    if (prevStep.current === step) return;
    prevStep.current = step;
    requestAnimationFrame(() => document.getElementById(step === 1 ? 'suEmail' : 'suName')?.focus({ preventScroll: true }));
  }, [step]);

  const go = (to: 1 | 2) => {
    setError('');
    if (to === 2) { pushed.current = true; setParams({ step: '2' }); return; }
    // 앞으로 올 때 쌓은 기록이 있으면 브라우저 '뒤로'와 똑같이 — 그래야 다시 '뒤로'를 눌러도 2단계로 돌아오지 않는다
    if (pushed.current) { pushed.current = false; nav(-1); } else setParams({}, { replace: true });
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
      rememberLoginEmail(email.trim());
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
      icon={step === 1 ? 'user' : 'music'} step={{ at: step, of: steps, onBack: step === 2 && !busy ? () => go(1) : undefined }}
      title={step === 1 ? 'AUDENIQ과 함께 시작해요.' : '어떤 이름으로 활동하나요?'}
      sub={step === 2 ? `가입 이메일 · ${email.trim()}` : ''}
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
            <label htmlFor="suPassword">비밀번호</label>
            <PasswordInput
              id="suPassword" required autoComplete="new-password" minLength={PASSWORD_MIN}
              aria-describedby={fe.password ? 'suPasswordErr' : 'suPwHelp'}
              aria-invalid={!!fe.password} enterKeyHint="next"
              value={password} onChange={e => { setPassword(e.target.value); clear('password'); }} onKeyDown={nextOnEnter('suConfirm')}
              show={showPw} onToggle={() => setShowPw(v => !v)}
            />
            <FieldError id="suPasswordErr">{fe.password}</FieldError>
            {/* 칸 아래 한 줄 — 입력 전에는 조건(12자 이상), 입력하면 같은 자리에 차오르는 강도 막대. 줄 높이가 같아 카드가 늘지 않는다 */}
            {!fe.password && (password ? (
              <div className="aq-pw-meter" data-score={pw.score} id="suPwHelp" aria-live="polite">
                <span><i /></span>
                <small className={longEnough ? 'is-ok' : undefined}>{longEnough && <CheckIcon size={12} />}{pw.label}</small>
              </div>
            ) : (
              <p className="ui-field-help aq-pw-rule" id="suPwHelp">{PASSWORD_MIN}자 이상</p>
            ))}
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
              placeholder="음원 플랫폼에 보일 이름" aria-invalid={!!fe.name} aria-describedby={fe.name ? 'suNameErr' : undefined}
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
        </form>
      )}
      {/* 로그인으로 가는 길은 첫 단계에만 — 2단계에는 위 단계 막대 옆 '이전' 화살표가 있다 */}
      {step === 1 && (
        <div className="auth-links">
          <span>이미 계정이 있으신가요?</span>
          <Link to="/login">로그인</Link>
        </div>
      )}
    </AuthLayout>
  );
}
