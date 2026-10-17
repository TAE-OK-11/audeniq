import { useEffect, useState } from 'react';
import { Link, useNavigate } from '../lib/router';
import { useAuth } from '../api/auth';
import { PASSWORD_MIN } from '../api/client';
import { authErrorMessage, messageForCode } from '../api/errors';
import { SocialSignIn } from '../components/SocialSignIn';
import { takeOAuthResult } from '../api/oauth';
import { AUTO_FOCUS, AuthLayout, EMAIL_INPUT, EmailSuggest, FieldError, PasswordInput, focusFirst, isEmail, nextOnEnter } from '../components/AuthLayout';
import { ApiError } from '../api/errors';
import { CheckIcon } from '../components/Check';
import { patchProfile } from '../store/profile';

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
  const [name, setName] = useState('');
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [confirm, setConfirm] = useState('');
  const [emailDone, setEmailDone] = useState(false);
  const [confirmDone, setConfirmDone] = useState(false);
  // 이미 가입된 이메일 — 오류 안에 '이 이메일로 로그인'을 둔다
  const [taken, setTaken] = useState(false);
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
  // 확인 칸은 한 글자만 쳐도 '일치하지 않아요'가 떴다 — 다 입력했거나(길이가 같아짐) 칸을 떠났을 때만 판단한다
  const confirmReady = !!confirm && (confirmDone || confirm.length >= password.length);
  const mismatch = confirmReady && password !== confirm;
  const matched = confirmReady && !!password && password === confirm;
  const longEnough = password.length >= PASSWORD_MIN;

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (busy) return;
    setError('');
    setTaken(false);
    // 브라우저 기본 경고(영어 말풍선) 대신 칸마다 한국어 안내를 한 번에 보여 주고 첫 칸으로 옮긴다
    const next: Fe = {
      name: !name.trim() ? '활동명을 입력해 주세요.' : undefined,
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
      setTaken(err instanceof ApiError && err.code === 'CONFLICT');
      setBusy(false);
    }
  }

  return (
    <AuthLayout icon="user" title="AUDENIQ과 함께 시작해요." sub="아티스트 계정을 만들고 발매를 준비해 보세요.">
      {error && (
        <div className="ui-callout is-danger aq-shake" role="alert" key={error}>
          {error}
          {taken && <> <Link to="/login" state={{ email: email.trim() }} className="aq-inline-link">이 이메일로 로그인</Link></>}
        </div>
      )}
      <form onSubmit={submit} noValidate>
        <div className="ui-field">
          {/* 활동명은 실명이 아니라 플랫폼에 보일 이름 — 자동 완성이 실명을 채우지 않게 nickname */}
          <label htmlFor="suName">활동명</label>
          <input
            id="suName" type="text" required autoComplete="nickname" autoFocus={AUTO_FOCUS} maxLength={120} enterKeyHint="next"
            placeholder="플랫폼에 보일 아티스트 이름" aria-invalid={!!fe.name} aria-describedby={fe.name ? 'suNameErr' : undefined}
            value={name} onChange={e => { setName(e.target.value); clear('name'); }} onKeyDown={nextOnEnter('suEmail')}
          />
          <FieldError id="suNameErr">{fe.name}</FieldError>
        </div>
        <div className="ui-field">
          <label htmlFor="suEmail">이메일</label>
          <input
            id="suEmail" {...EMAIL_INPUT} required autoComplete="email"
            placeholder="name@example.com" aria-invalid={!!fe.email} aria-describedby={fe.email ? 'suEmailErr' : undefined}
            value={email} onChange={e => { setEmail(e.target.value); setEmailDone(false); clear('email'); }}
            onBlur={() => setEmailDone(true)} onKeyDown={nextOnEnter('suPassword')}
          />
          <FieldError id="suEmailErr">{fe.email}</FieldError>
          {emailDone && !fe.email && <EmailSuggest value={email} onApply={setEmail} />}
        </div>
        <div className="ui-field">
          <label htmlFor="suPassword">비밀번호</label>
          <PasswordInput
            id="suPassword" required autoComplete="new-password" minLength={PASSWORD_MIN}
            aria-describedby={fe.password ? 'suPasswordErr' : 'suPwRule suPwHelp'}
            aria-invalid={!!fe.password} enterKeyHint="next"
            value={password} onChange={e => { setPassword(e.target.value); clear('password'); }} onKeyDown={nextOnEnter('suConfirm')}
            show={showPw} onToggle={() => setShowPw(v => !v)}
          />
          <FieldError id="suPasswordErr">{fe.password}</FieldError>
          {/* 조건은 입력 전부터 보이게 — 칸 안 안내 글자는 치기 시작하면 사라졌다. 채우면 초록 체크 */}
          {!fe.password && (
            <p className={`ui-field-help aq-pw-rule${longEnough ? ' is-ok' : ''}`} id="suPwRule">
              {longEnough && <CheckIcon size={12} />}{PASSWORD_MIN}자 이상
            </p>
          )}
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
            placeholder="한 번 더 입력해 주세요" aria-invalid={mismatch || !!fe.confirm} enterKeyHint="done"
            aria-describedby={fe.confirm || mismatch ? 'suConfirmErr' : matched ? 'suConfirmOk' : undefined}
            value={confirm} onChange={e => { setConfirm(e.target.value); setConfirmDone(false); clear('confirm'); }}
            onBlur={() => setConfirmDone(true)}
          />
          <FieldError id="suConfirmErr">{fe.confirm ?? (mismatch ? '비밀번호가 일치하지 않아요.' : undefined)}</FieldError>
          {matched && !fe.confirm && <p className="ui-field-help aq-pw-rule is-ok" id="suConfirmOk"><CheckIcon size={12} />비밀번호가 일치해요</p>}
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
          // 맨 위 빨간 상자 대신 동의할 칸으로 옮겨 그 아래에 알려 준다
          setFe(f => ({ ...f, terms: '이용약관에 동의한 뒤 Google·Apple로 계속할 수 있어요.' }));
          focusFirst(['suTerms']);
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

