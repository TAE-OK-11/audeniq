// 회원가입 직후 한 번 — 가입할 때 적은 휴대폰 번호로 인증번호를 보내 확인한다 (비밀번호 찾기에 쓴다).
// 문자를 보낼 수 없으면(문자 서비스 준비 전·체험 모드) 묻지 않고 다음 화면으로. 나중에 계정 정보에서 등록할 수 있다.
import { useEffect, useRef, useState } from 'react';
import { Navigate, useNavigate } from '../lib/router';
import { AuthLayout } from '../components/AuthLayout';
import { SmsCodeInput, useCountdown } from '../components/SmsCodeInput';
import { useToast } from '../components/Toast';
import { errorMessage } from '../api/errors';
import { startPhoneRegistration, verifyPhoneRegistration, type SmsChallenge } from '../api/recovery';
import { clearPhoneHandoff, peekPhoneHandoff } from '../lib/phoneHandoff';
import { hasPasskeyHandoff } from '../lib/passkeyHandoff';
import { patchAccount } from '../store/account';

export function PhoneSetup() {
  const nav = useNavigate();
  const toast = useToast();
  const [phone] = useState(peekPhoneHandoff);
  const [challenge, setChallenge] = useState<SmsChallenge | null>(null);
  const [code, setCode] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const left = useCountdown(challenge?.expiresAt ?? '');
  const started = useRef(false);

  // 다음은 패스키 만들기(가입 마지막 단계) 또는 홈
  const next = () => {
    clearPhoneHandoff();
    nav(hasPasskeyHandoff() ? '/passkey-setup' : '/', { replace: true });
  };

  useEffect(() => {
    if (!phone || started.current) return;
    started.current = true;
    startPhoneRegistration(phone).then(setChallenge, next);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [phone]);

  // 새로고침 등으로 넘겨받은 번호가 없으면 묻지 않고 넘어간다
  if (!phone) return <Navigate to={hasPasskeyHandoff() ? '/passkey-setup' : '/'} replace />;
  if (!challenge) return (
    <div className="aq-boot" role="status" aria-label="불러오는 중">
      <img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="" />
      <span className="aq-boot-bar" />
    </div>
  );

  const resend = async () => {
    if (busy) return;
    setError('');
    setBusy(true);
    try {
      setChallenge(await startPhoneRegistration(phone));
      setCode('');
    } catch (err) {
      setError(errorMessage(err, '인증번호를 보내지 못했어요. 잠시 후 다시 시도해 주세요.'));
    } finally {
      setBusy(false);
    }
  };

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busy) return;
    if (left === 0) return setError('인증 시간이 지났어요. 인증번호를 다시 받아 주세요.');
    if (code.length !== 6) return setError('인증번호 6자리를 입력해 주세요.');
    setError('');
    setBusy(true);
    try {
      patchAccount({ phone: await verifyPhoneRegistration(challenge.id, code) });
      toast('휴대폰 번호를 확인했어요.', 'success');
      next();
    } catch (err) {
      setError(errorMessage(err, '인증번호를 확인하지 못했어요. 잠시 후 다시 시도해 주세요.'));
      setCode('');
      setBusy(false);
    }
  };

  return (
    <AuthLayout icon="lock" title="휴대폰 번호를 확인해 주세요" sub="비밀번호를 잊었을 때 이 번호로 다시 정할 수 있어요.">
      {error && <div className="ui-callout is-danger aq-shake" role="alert" key={error}>{error}</div>}
      <form className="aq-verify" onSubmit={submit} noValidate>
        <p className="aq-verify-lead">{phone}로 보낸 인증번호 6자리를 입력해 주세요.</p>
        <SmsCodeInput id="phoneSetupCode" value={code} onChange={v => { setCode(v); setError(''); }} left={left} onResend={() => void resend()} resending={busy} />
        <button className={`ui-btn is-primary auth-submit${busy ? ' is-busy' : ''}`} type="submit" disabled={busy} aria-busy={busy}>
          {busy ? '확인하는 중' : '확인'}
        </button>
        <button type="button" className="aq-verify-resend" onClick={next} disabled={busy}>나중에 계정 정보에서 하기</button>
      </form>
    </AuthLayout>
  );
}
