// 휴대폰 번호 등록 — 문자 인증번호로 확인한 번호만 저장한다. 비밀번호를 잊었을 때 이 번호로 재설정한다.
import { useState } from 'react';
import { useToast } from './Toast';
import { useModalClose } from './Modal';
import { FieldError } from './AuthLayout';
import { SmsCodeInput, useCountdown } from './SmsCodeInput';
import { errorMessage } from '../api/errors';
import { startPhoneRegistration, verifyPhoneRegistration, type SmsChallenge } from '../api/recovery';
import { patchAccount, useAccount } from '../store/account';
import { formatPhone, isPhone } from '../lib/phone';

export function PhoneSettings() {
  const toast = useToast();
  const close = useModalClose();
  const current = useAccount().phone;
  const [phone, setPhone] = useState('');
  const [challenge, setChallenge] = useState<SmsChallenge | null>(null);
  const [code, setCode] = useState('');
  const [fieldErr, setFieldErr] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const left = useCountdown(challenge?.expiresAt ?? '');

  const send = async () => {
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

  const submitPhone = (e: React.FormEvent) => {
    e.preventDefault();
    if (busy) return;
    const fe = !phone ? '휴대폰 번호를 입력해 주세요.' : !isPhone(phone) ? '휴대폰 번호를 확인해 주세요. (예: 010-1234-5678)' : '';
    setFieldErr(fe);
    if (!fe) void send();
  };

  const submitCode = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busy || !challenge) return;
    if (left === 0) return setError('인증 시간이 지났어요. 인증번호를 다시 받아 주세요.');
    if (code.length !== 6) return setError('인증번호 6자리를 입력해 주세요.');
    setError('');
    setBusy(true);
    try {
      const r = await verifyPhoneRegistration(challenge.id, code);
      patchAccount({ phone: r });
      toast('휴대폰 번호를 등록했어요.');
      close();
    } catch (err) {
      setError(errorMessage(err, '인증번호를 확인하지 못했어요. 잠시 후 다시 시도해 주세요.'));
      setCode('');
      setBusy(false);
    }
  };

  const errorBox = error && <div className="ui-callout is-danger aq-shake" role="alert" key={error}>{error}</div>;

  if (challenge) {
    return (
      <form className="aq-verify" onSubmit={submitCode} noValidate>
        {errorBox}
        <p className="aq-verify-lead">{phone}로 보낸 인증번호 6자리를 입력해 주세요.</p>
        <SmsCodeInput
          id="phoneCode" value={code} onChange={v => { setCode(v); setError(''); }}
          left={left} onResend={() => void send()} resending={busy}
        />
        <button type="submit" className={`ui-btn is-primary is-block${busy ? ' is-busy' : ''}`} disabled={busy}>{busy ? '확인하는 중' : '확인'}</button>
        <button type="button" className="aq-verify-resend" onClick={() => { setChallenge(null); setError(''); }}>번호 다시 입력</button>
      </form>
    );
  }

  return (
    <form className="aq-verify" onSubmit={submitPhone} noValidate>
      {errorBox}
      <p className="aq-verify-lead">
        {current && !current.verifiedAt
          ? `가입할 때 적은 ···${current.last4}는 아직 확인 전이에요. 문자 인증번호로 확인해야 비밀번호 찾기에 쓸 수 있어요.`
          : current
          ? `지금 등록된 번호는 ···${current.last4}예요. 바꾸려면 새 번호로 인증해 주세요.`
          : '비밀번호를 잊었을 때 이 번호로 받은 문자 인증번호로 다시 정할 수 있어요.'}
      </p>
      <div className="ui-field">
        <label htmlFor="phoneNumber">휴대폰 번호</label>
        <input
          id="phoneNumber" type="tel" inputMode="numeric" required autoComplete="tel-national" enterKeyHint="send" maxLength={13}
          placeholder="010-1234-5678" aria-invalid={!!fieldErr} aria-describedby={fieldErr ? 'phoneNumberErr' : undefined}
          value={phone} onChange={e => { setPhone(formatPhone(e.target.value)); setFieldErr(''); }}
        />
        <FieldError id="phoneNumberErr">{fieldErr}</FieldError>
      </div>
      <button type="submit" className={`ui-btn is-primary is-block${busy ? ' is-busy' : ''}`} disabled={busy}>{busy ? '보내는 중' : '인증번호 받기'}</button>
    </form>
  );
}
