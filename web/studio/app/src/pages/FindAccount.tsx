import { useState } from 'react';
import { Link, useNavigate } from '../lib/router';
import { AUTO_FOCUS, AuthLayout, EMAIL_INPUT, FieldError, PasswordInput, focusFirst, isEmail, nextOnEnter } from '../components/AuthLayout';
import { SUPPORT_EMAIL } from '../lib/support';
import { Segmented } from '../components/Segmented';
import { Glyph } from '../components/Glyph';
import { SmsCodeInput, useCountdown } from '../components/SmsCodeInput';
import { useAuth } from '../api/auth';
import { MOCK, PASSWORD_MIN } from '../api/client';
import { passkeySupported } from '../api/passkey';
import { ApiError, authErrorMessage, errorMessage } from '../api/errors';
import { completePasswordReset, startFindEmail, startPasswordReset, verifyFindEmail, verifyPasswordReset, type SmsChallenge } from '../api/recovery';
import { formatPhone, isPhone } from '../lib/phone';
import { lastLoginEmail } from '../lib/lastEmail';

// 이메일 찾기·비밀번호 재설정 모두 계정에 등록(문자 인증)한 휴대폰 번호로 한다. 번호를 등록하지 않았으면 고객지원으로.
const supportMail = (tab: 'id' | 'password') => {
  const subject = tab === 'id' ? '가입한 이메일 찾기 요청' : '비밀번호 재설정 요청';
  const body = tab === 'id'
    ? '활동명:\n가입할 때 쓴 것 같은 이메일(아는 만큼):\n연락받을 이메일:\n'
    : '가입한 이메일:\n활동명:\n';
  return `mailto:${SUPPORT_EMAIL}?subject=${encodeURIComponent(subject)}&body=${encodeURIComponent(body)}`;
};

/** 문자 인증을 쓸 수 없는 상태 — 고객지원으로 안내한다 */
const SMS_UNAVAILABLE = new Set(['SMS_PROVIDER_NOT_CONFIGURED', 'SMS_UNSUPPORTED']);
const codeOf = (e: unknown) => (e instanceof ApiError ? e.code : '');

/** 비밀번호 재설정 — 이메일·휴대폰 번호 → 문자 인증번호 → 새 비밀번호 */
function PasswordReset() {
  const nav = useNavigate();
  const [step, setStep] = useState<'start' | 'code' | 'password' | 'done'>('start');
  const [email, setEmail] = useState(lastLoginEmail);
  const [phone, setPhone] = useState('');
  const [challenge, setChallenge] = useState<SmsChallenge | null>(null);
  const [code, setCode] = useState('');
  const [token, setToken] = useState('');
  const [password, setPassword] = useState('');
  const [confirm, setConfirm] = useState('');
  const [showPw, setShowPw] = useState(false);
  const [fe, setFe] = useState<{ email?: string; phone?: string; password?: string; confirm?: string }>({});
  const [error, setError] = useState('');
  const [unavailable, setUnavailable] = useState(false);
  const [busy, setBusy] = useState(false);
  const left = useCountdown(challenge?.expiresAt ?? '');

  const fail = (err: unknown, fallback: string) => {
    setUnavailable(SMS_UNAVAILABLE.has(codeOf(err)));
    setError(errorMessage(err, fallback));
  };

  const send = async (): Promise<boolean> => {
    setError('');
    setBusy(true);
    try {
      setChallenge(await startPasswordReset(email.trim(), phone));
      setCode('');
      return true;
    } catch (err) {
      fail(err, '인증번호를 보내지 못했어요. 잠시 후 다시 시도해 주세요.');
      return false;
    } finally {
      setBusy(false);
    }
  };

  const submitStart = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busy) return;
    const n = {
      email: !email.trim() ? '이메일을 입력해 주세요.' : !isEmail(email) ? '이메일 주소를 확인해 주세요.' : undefined,
      phone: !phone ? '휴대폰 번호를 입력해 주세요.' : !isPhone(phone) ? '휴대폰 번호를 확인해 주세요. (예: 010-1234-5678)' : undefined,
    };
    setFe(n);
    if (n.email || n.phone) { focusFirst([n.email && 'resetEmail', n.phone && 'resetPhone']); return; }
    if (await send()) setStep('code');
  };

  const submitCode = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busy || !challenge) return;
    if (left === 0) return setError('인증 시간이 지났어요. 인증번호를 다시 받아 주세요.');
    if (code.length !== 6) return setError('인증번호 6자리를 입력해 주세요.');
    setError('');
    setBusy(true);
    try {
      setToken(await verifyPasswordReset(challenge.id, code));
      setStep('password');
    } catch (err) {
      fail(err, '인증번호를 확인하지 못했어요. 잠시 후 다시 시도해 주세요.');
      setCode('');
    } finally {
      setBusy(false);
    }
  };

  const submitPassword = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busy) return;
    const n = {
      password: password.length < PASSWORD_MIN ? `비밀번호는 ${PASSWORD_MIN}자 이상이어야 해요.` : password.length > 128 ? '비밀번호는 128자까지 쓸 수 있어요.' : undefined,
      confirm: confirm !== password ? '비밀번호가 일치하지 않아요.' : undefined,
    };
    setFe(n);
    if (n.password || n.confirm) { focusFirst([n.password && 'resetPassword', n.confirm && 'resetConfirm']); return; }
    setError('');
    setBusy(true);
    try {
      await completePasswordReset(token, password);
      setPassword('');
      setConfirm('');
      setStep('done');
    } catch (err) {
      const c = codeOf(err);
      if (c === 'PASSWORD_TOO_WEAK') { setFe({ password: errorMessage(err) }); focusFirst(['resetPassword']); }
      else {
        // 재설정 시간이 지났으면 처음부터 — 인증번호를 다시 받아야 한다
        if (c === 'RESET_TOKEN_INVALID') { setToken(''); setChallenge(null); setStep('start'); }
        fail(err, '비밀번호를 바꾸지 못했어요. 잠시 후 다시 시도해 주세요.');
      }
    } finally {
      setBusy(false);
    }
  };

  const errorBox = error && (
    <div className="ui-callout is-danger aq-shake" role="alert" key={error}>
      {error}
      {unavailable && <> <a href={supportMail('password')} className="aq-inline-link">고객지원에 이메일 보내기</a></>}
    </div>
  );

  if (step === 'done') {
    return (
      <div className="aq-find-panel" role="tabpanel">
        <div className="ui-callout is-success" role="status">비밀번호를 바꿨어요. 보안을 위해 모든 기기에서 로그아웃했어요.</div>
        <button type="button" className="ui-btn is-primary auth-submit" onClick={() => nav('/login', { replace: true, state: { email: email.trim() } })}>
          새 비밀번호로 로그인
        </button>
      </div>
    );
  }

  if (step === 'password') {
    return (
      <form className="aq-find-panel" role="tabpanel" onSubmit={submitPassword} noValidate>
        {errorBox}
        <p className="aq-muted">새로 쓸 비밀번호를 정해 주세요. 바꾸면 모든 기기에서 로그아웃돼요.</p>
        {/* 비밀번호 관리자가 어느 계정의 비밀번호인지 알도록 */}
        <input type="email" autoComplete="username" value={email.trim()} readOnly hidden />
        <div className="ui-field">
          <label htmlFor="resetPassword">새 비밀번호</label>
          <PasswordInput
            id="resetPassword" required autoComplete="new-password" minLength={PASSWORD_MIN} autoFocus={AUTO_FOCUS}
            aria-invalid={!!fe.password} aria-describedby={fe.password ? 'resetPasswordErr' : 'resetPwHelp'} enterKeyHint="next"
            value={password} onChange={e => { setPassword(e.target.value); setFe(f => ({ ...f, password: undefined })); }}
            onKeyDown={nextOnEnter('resetConfirm')} show={showPw} onToggle={() => setShowPw(v => !v)}
          />
          <FieldError id="resetPasswordErr">{fe.password}</FieldError>
          {!fe.password && <p className="ui-field-help aq-pw-rule" id="resetPwHelp">{PASSWORD_MIN}자 이상</p>}
        </div>
        <div className="ui-field">
          <label htmlFor="resetConfirm">새 비밀번호 확인</label>
          <input
            id="resetConfirm" type={showPw ? 'text' : 'password'} required autoComplete="new-password" enterKeyHint="done"
            placeholder="한 번 더 입력해 주세요" aria-invalid={!!fe.confirm} aria-describedby={fe.confirm ? 'resetConfirmErr' : undefined}
            value={confirm} onChange={e => { setConfirm(e.target.value); setFe(f => ({ ...f, confirm: undefined })); }}
          />
          <FieldError id="resetConfirmErr">{fe.confirm}</FieldError>
        </div>
        <button type="submit" className={`ui-btn is-primary auth-submit${busy ? ' is-busy' : ''}`} disabled={busy} aria-busy={busy}>
          {busy ? '바꾸는 중' : '비밀번호 바꾸기'}
        </button>
      </form>
    );
  }

  if (step === 'code' && challenge) {
    return (
      <form className="aq-find-panel aq-verify" role="tabpanel" onSubmit={submitCode} noValidate>
        {errorBox}
        {/* 계정이 없거나 번호가 달라도 서버는 같은 답을 준다 — 보냈다고 단정하지 않는다 */}
        <p className="aq-verify-lead">
          계정에 등록된 번호가 맞다면 ···{challenge.phoneLast4}로 인증번호 6자리를 보냈어요.
        </p>
        <SmsCodeInput
          id="resetCode" value={code} onChange={v => { setCode(v); setError(''); }}
          left={left} onResend={() => void send()} resending={busy}
        />
        <button type="submit" className={`ui-btn is-primary auth-submit${busy ? ' is-busy' : ''}`} disabled={busy} aria-busy={busy}>
          {busy ? '확인하는 중' : '확인'}
        </button>
        <button type="button" className="aq-verify-resend" onClick={() => { setStep('start'); setError(''); }}>이메일·휴대폰 번호 다시 입력</button>
      </form>
    );
  }

  return (
    <form className="aq-find-panel" role="tabpanel" onSubmit={submitStart} noValidate>
      {errorBox}
      <p className="aq-muted">가입한 이메일과 계정에 등록한 휴대폰 번호를 입력하면 문자로 인증번호를 보내 드려요.</p>
      <div className="ui-field">
        <label htmlFor="resetEmail">이메일</label>
        <input
          id="resetEmail" {...EMAIL_INPUT} required autoComplete="username" autoFocus={AUTO_FOCUS && !email}
          placeholder="name@example.com" aria-invalid={!!fe.email} aria-describedby={fe.email ? 'resetEmailErr' : undefined}
          value={email} onChange={e => { setEmail(e.target.value); setFe(f => ({ ...f, email: undefined })); }}
          onKeyDown={nextOnEnter('resetPhone')}
        />
        <FieldError id="resetEmailErr">{fe.email}</FieldError>
      </div>
      <div className="ui-field">
        <label htmlFor="resetPhone">휴대폰 번호</label>
        <input
          id="resetPhone" type="tel" inputMode="numeric" required autoComplete="tel-national" enterKeyHint="send" maxLength={13}
          autoFocus={AUTO_FOCUS && !!email} placeholder="010-1234-5678"
          aria-invalid={!!fe.phone} aria-describedby={fe.phone ? 'resetPhoneErr' : undefined}
          value={phone} onChange={e => { setPhone(formatPhone(e.target.value)); setFe(f => ({ ...f, phone: undefined })); }}
        />
        <FieldError id="resetPhoneErr">{fe.phone}</FieldError>
      </div>
      <button type="submit" className={`ui-btn is-primary auth-submit${busy ? ' is-busy' : ''}`} disabled={busy} aria-busy={busy}>
        {busy ? '보내는 중' : '인증번호 받기'}
      </button>
      {/* 휴대폰 번호는 로그인한 뒤 계정 정보에서 등록한다 — 등록하지 않았다면 고객지원으로 */}
      <p className="aq-muted">
        휴대폰 번호를 등록하지 않았다면 <a href={supportMail('password')} className="aq-inline-link">고객지원</a>에 문의해 주세요.
      </p>
    </form>
  );
}

/** 이메일 찾기 — 휴대폰 번호 → 문자 인증번호 → 그 번호로 가입한 이메일(앞부분 가림) */
function FindEmail({ onReset }: { onReset: () => void }) {
  const [phone, setPhone] = useState('');
  const [challenge, setChallenge] = useState<SmsChallenge | null>(null);
  const [code, setCode] = useState('');
  const [emails, setEmails] = useState<string[] | null>(null);
  const [fe, setFe] = useState('');
  const [error, setError] = useState('');
  const [unavailable, setUnavailable] = useState(false);
  const [busy, setBusy] = useState(false);
  const left = useCountdown(challenge?.expiresAt ?? '');

  const fail = (err: unknown, fallback: string) => {
    setUnavailable(SMS_UNAVAILABLE.has(codeOf(err)));
    setError(errorMessage(err, fallback));
  };

  const send = async () => {
    setError('');
    setBusy(true);
    try {
      setChallenge(await startFindEmail(phone));
      setCode('');
    } catch (err) {
      fail(err, '인증번호를 보내지 못했어요. 잠시 후 다시 시도해 주세요.');
    } finally {
      setBusy(false);
    }
  };

  const submitPhone = (e: React.FormEvent) => {
    e.preventDefault();
    if (busy) return;
    const n = !phone ? '휴대폰 번호를 입력해 주세요.' : !isPhone(phone) ? '휴대폰 번호를 확인해 주세요. (예: 010-1234-5678)' : '';
    setFe(n);
    if (n) { focusFirst(['findPhone']); return; }
    void send();
  };

  const submitCode = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busy || !challenge) return;
    if (left === 0) return setError('인증 시간이 지났어요. 인증번호를 다시 받아 주세요.');
    if (code.length !== 6) return setError('인증번호 6자리를 입력해 주세요.');
    setError('');
    setBusy(true);
    try {
      setEmails(await verifyFindEmail(challenge.id, code));
    } catch (err) {
      fail(err, '인증번호를 확인하지 못했어요. 잠시 후 다시 시도해 주세요.');
      setCode('');
    } finally {
      setBusy(false);
    }
  };

  const errorBox = error && (
    <div className="ui-callout is-danger aq-shake" role="alert" key={error}>
      {error}
      {unavailable && <> <a href={supportMail('id')} className="aq-inline-link">고객지원에 이메일 보내기</a></>}
    </div>
  );

  if (emails) {
    return (
      <div className="aq-find-panel" role="tabpanel">
        {emails.length ? (
          <>
            <p className="aq-muted">이 번호로 가입한 이메일이에요. 개인정보 보호를 위해 앞부분 일부만 보여 드려요.</p>
            <ul className="aq-found-emails">
              {emails.map(m => <li key={m}><Glyph name="mail" size={16} /><strong>{m}</strong></li>)}
            </ul>
            <Link to="/login" className="ui-btn is-primary auth-submit">로그인하기</Link>
            <button type="button" className="aq-verify-resend" onClick={onReset}>비밀번호가 기억나지 않아요</button>
          </>
        ) : (
          <>
            <p className="aq-muted">이 번호로 확인된 계정을 찾지 못했어요. 고객지원에 문의해 주세요.</p>
            <a href={supportMail('id')} className="ui-btn is-primary auth-submit">고객지원에 이메일 보내기</a>
          </>
        )}
      </div>
    );
  }

  if (challenge) {
    return (
      <form className="aq-find-panel aq-verify" role="tabpanel" onSubmit={submitCode} noValidate>
        {errorBox}
        {/* 그 번호로 가입한 계정이 없어도 서버는 같은 답을 준다 — 보냈다고 단정하지 않는다 */}
        <p className="aq-verify-lead">이 번호로 가입한 계정이 있다면 ···{challenge.phoneLast4}로 인증번호 6자리를 보냈어요.</p>
        <SmsCodeInput id="findCode" value={code} onChange={v => { setCode(v); setError(''); }} left={left} onResend={() => void send()} resending={busy} />
        <button type="submit" className={`ui-btn is-primary auth-submit${busy ? ' is-busy' : ''}`} disabled={busy} aria-busy={busy}>
          {busy ? '확인하는 중' : '확인'}
        </button>
        <button type="button" className="aq-verify-resend" onClick={() => { setChallenge(null); setError(''); }}>휴대폰 번호 다시 입력</button>
      </form>
    );
  }

  return (
    <form className="aq-find-panel" role="tabpanel" onSubmit={submitPhone} noValidate>
      {errorBox}
      <p className="aq-muted">계정에 등록한 휴대폰 번호를 입력하면 문자로 인증번호를 보내 드려요.</p>
      <div className="ui-field">
        <label htmlFor="findPhone">휴대폰 번호</label>
        <input
          id="findPhone" type="tel" inputMode="numeric" required autoComplete="tel-national" enterKeyHint="send" maxLength={13}
          autoFocus={AUTO_FOCUS} placeholder="010-1234-5678" aria-invalid={!!fe} aria-describedby={fe ? 'findPhoneErr' : undefined}
          value={phone} onChange={e => { setPhone(formatPhone(e.target.value)); setFe(''); }}
        />
        <FieldError id="findPhoneErr">{fe}</FieldError>
      </div>
      <button type="submit" className={`ui-btn is-primary auth-submit${busy ? ' is-busy' : ''}`} disabled={busy} aria-busy={busy}>
        {busy ? '보내는 중' : '인증번호 받기'}
      </button>
      <p className="aq-muted">
        휴대폰 번호를 등록하지 않았다면 <a href={supportMail('id')} className="aq-inline-link">고객지원</a>에 문의해 주세요.
      </p>
    </form>
  );
}

export function FindAccount() {
  const { loginPasskey } = useAuth();
  const nav = useNavigate();
  const [tab, setTab] = useState<'id' | 'password'>('id');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const canPasskey = !MOCK && passkeySupported();

  // 이메일·비밀번호가 기억나지 않아도 이 기기에 패스키가 있으면 그것으로 바로 — 고객지원을 기다리지 않게 먼저 권한다
  const withPasskey = async () => {
    if (busy) return;
    setBusy(true);
    setError('');
    try {
      await loginPasskey('');
      nav('/', { replace: true });
    } catch (err) {
      setError(err instanceof ApiError && err.code.startsWith('PASSKEY') ? err.message : authErrorMessage(err, 'login'));
      setBusy(false);
    }
  };

  return (
    <AuthLayout
      icon="info" title="계정을 찾고 있나요?"
      sub={tab === 'id' ? '등록한 휴대폰 번호로 가입한 이메일을 찾을 수 있어요.' : '등록한 휴대폰 번호로 비밀번호를 다시 정할 수 있어요.'}
    >
      {error && <div className="ui-callout is-danger aq-shake" role="alert" key={error}>{error}</div>}
      <Segmented
        tabs className="aq-seg-tabs" label="계정 찾기 구분" value={tab} onChange={setTab}
        options={[{ value: 'id', label: '이메일 찾기' }, { value: 'password', label: '비밀번호 찾기' }] as const}
      />
      {tab === 'id' ? <FindEmail onReset={() => setTab('password')} /> : <PasswordReset />}
      {/* 이 기기에 패스키가 있으면 이메일·비밀번호 없이 바로 — '또는'으로 조용히 */}
      {canPasskey && (
        <div className="aq-passkey-alt">
          <div className="aq-social-divider" role="separator"><span>또는</span></div>
          <button type="button" className={`ui-btn is-soft auth-submit${busy ? ' is-busy' : ''}`} onClick={() => void withPasskey()} disabled={busy}>
            <Glyph name="key" size={18} />
            <span>패스키로 바로 로그인</span>
          </button>
        </div>
      )}

      <div className="auth-links">
        <Link to="/login">로그인</Link>
        <span aria-hidden="true">·</span>
        <Link to="/signup">회원가입</Link>
      </div>
    </AuthLayout>
  );
}
