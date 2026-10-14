// 2단계 인증 화면 — 관리자 콘솔은 개인정보를 다루므로 비밀번호에 더해 인증 앱의 6자리 코드를 받는다.
// 처음이면 인증 앱에 설정 키를 등록하고, 첫 코드를 확인하면 등록이 끝난다.
import { useEffect, useState } from 'react';
import { mfaApi, type MfaEnrollment } from '../api/mfa';
import { ApiError } from '../api/errors';

export function MfaGate({ email, onDone, onLogout }: { email?: string; onDone: () => void; onLogout: () => void }) {
  const [enroll, setEnroll] = useState<MfaEnrollment | null>(null);
  const [phase, setPhase] = useState<'loading' | 'code' | 'error'>('loading');
  const [code, setCode] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let alive = true;
    mfaApi.status()
      .then(async s => {
        if (s.verified) { onDone(); return; }
        if (!s.enrolled) {
          const e = await mfaApi.enroll();
          if (alive) setEnroll(e);
        }
        if (alive) setPhase('code');
      })
      .catch(e => { if (alive) { setError(e instanceof ApiError ? e.message : ''); setPhase('error'); } });
    return () => { alive = false; };
  }, [onDone]);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (busy) return;
    setBusy(true);
    setError('');
    try {
      await mfaApi.verify(code.replace(/\s/g, ''));
      onDone();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : '확인하지 못했어요. 다시 시도해 주세요.');
      setCode('');
      setBusy(false);
    }
  }

  const grouped = enroll?.secret.match(/.{1,4}/g)?.join(' ') ?? '';
  return (
    <div className="auth-page">
      <div className="aq-auth-glow" aria-hidden="true" />
      <div className="auth-split">
        <div className="auth-card">
          <h1>2단계 인증</h1>
          {phase === 'loading' && <p className="auth-sub" role="status">확인하는 중이에요.</p>}
          {phase === 'error' && (
            <>
              <div className="ui-callout is-danger" role="alert">{error || '2단계 인증 정보를 불러오지 못했어요.'}</div>
              <button type="button" className="ui-btn auth-submit" onClick={onLogout}>다시 로그인</button>
            </>
          )}
          {phase === 'code' && (
            <>
              {enroll ? (
                <>
                  <p className="auth-sub">
                    관리자 콘솔은 개인정보를 다루므로 인증 앱 코드가 필요해요. Google Authenticator·Microsoft Authenticator 같은 앱에서
                    &lsquo;설정 키 입력&rsquo;을 고르고 아래 키를 넣어 주세요. 휴대폰에서 열었다면 링크를 눌러도 돼요.
                  </p>
                  <div className="ui-field">
                    <label htmlFor="mfaSecret">설정 키 ({email || '이 계정'})</label>
                    <input id="mfaSecret" readOnly value={grouped} onFocus={e => e.currentTarget.select()} spellCheck={false} autoComplete="off" />
                  </div>
                  <p className="auth-sub"><a href={enroll.otpauth_uri}>인증 앱에서 열기</a> · 시간 기반 · 6자리 · 30초</p>
                </>
              ) : (
                <p className="auth-sub">인증 앱에 표시된 6자리 코드를 입력해 주세요. 인증 앱을 잃어버렸다면 운영 책임자에게 초기화를 요청해 주세요.</p>
              )}
              {error && <div className="ui-callout is-danger aq-shake" role="alert" key={error}>{error}</div>}
              <form onSubmit={submit}>
                <div className="ui-field">
                  <label htmlFor="mfaCode">6자리 코드</label>
                  <input
                    id="mfaCode" required autoFocus inputMode="numeric" autoComplete="one-time-code" pattern="[0-9 ]{6,7}" maxLength={7}
                    placeholder="000000" value={code} onChange={e => setCode(e.target.value)}
                  />
                </div>
                <button className={`ui-btn is-primary auth-submit${busy ? ' is-busy' : ''}`} type="submit" disabled={busy} aria-busy={busy}>
                  {busy ? '확인하는 중' : enroll ? '등록하고 들어가기' : '확인'}
                </button>
              </form>
              <button type="button" className="ui-btn is-ghost auth-submit" onClick={onLogout}>다른 계정으로 로그인</button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
