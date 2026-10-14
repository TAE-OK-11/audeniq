import { loginRedirect } from './lib/loginRedirect';
// 로그인 — 스튜디오 로그인 화면(AuthLayout)과 같은 카드·입력창·버튼. 계정도 스튜디오와 같다.
import { useState } from 'react';
import { useLocation, useNavigate } from './lib/router';
import { useAuth } from './auth';
import { authErrorMessage } from './api/errors';
import { MOCK } from './lib/mode';
import { passkeySupported } from './api/passkey';
import { ApiError } from './api/errors';

function PasswordInput(props: React.InputHTMLAttributes<HTMLInputElement> & { show: boolean; onToggle: () => void }) {
  const { show, onToggle, ...rest } = props;
  return (
    <div className="aq-password">
      <input {...rest} type={show ? 'text' : 'password'} />
      <button type="button" className="aq-password-toggle" onClick={onToggle} aria-label={show ? '비밀번호 숨기기' : '비밀번호 보기'} aria-pressed={show}>
        {show ? '숨기기' : '보기'}
      </button>
    </div>
  );
}

export function Login() {
  const { login, loginPasskey } = useAuth();
  const nav = useNavigate();
  const loc = useLocation();
  const from = loginRedirect(loc.state);
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [showPw, setShowPw] = useState(false);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (busy) return;
    setError('');
    setBusy(true);
    try {
      await login(email, password);
      nav(from ?? '/', { replace: true });
    } catch (err) {
      setError(authErrorMessage(err, 'login'));
      setBusy(false);
    }
  }

  // 패스키(지문·얼굴·PIN)는 그 자체로 2단계 인증이라 인증 앱 코드를 따로 묻지 않는다.
  async function withPasskey() {
    if (busy) return;
    setError('');
    setBusy(true);
    try {
      await loginPasskey(email);
      nav(from ?? '/', { replace: true });
    } catch (err) {
      setError(err instanceof ApiError && err.code.startsWith('PASSKEY') ? err.message : authErrorMessage(err, 'login'));
      setBusy(false);
    }
  }

  return (
    <div className="auth-page">
      <div className="aq-auth-glow" aria-hidden="true" />
      <div className="auth-split">
        <div className="auth-card">
          <span className="aq-auth-brand" aria-label="AUDENIQ ADMIN">
            <img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" />
            <span>ADMIN</span>
          </span>
          <h1>관리자 콘솔</h1>
          <p className="auth-sub">
            {MOCK ? '체험 모드예요. 아무 이메일로나 들어가 예시 데이터를 볼 수 있어요.' : '스튜디오와 같은 계정으로 로그인해요.'}
          </p>
          {error && <div className="ui-callout is-danger aq-shake" role="alert" key={error}>{error}</div>}
          <form onSubmit={submit}>
            <div className="ui-field">
              <label htmlFor="loginEmail">이메일</label>
              <input
                id="loginEmail" type="email" required autoComplete="email" autoFocus
                placeholder="이메일을 입력해 주세요"
                value={email} onChange={e => setEmail(e.target.value)}
              />
            </div>
            <div className="ui-field">
              <label htmlFor="loginPassword">비밀번호</label>
              <PasswordInput
                id="loginPassword" required={!MOCK} autoComplete="current-password"
                placeholder="비밀번호를 입력해 주세요"
                value={password} onChange={e => setPassword(e.target.value)}
                show={showPw} onToggle={() => setShowPw(v => !v)}
              />
            </div>
            <button className={`ui-btn is-primary auth-submit${busy ? ' is-busy' : ''}`} type="submit" disabled={busy} aria-busy={busy}>
              {busy ? '들어가는 중' : '로그인'}
            </button>
            {!MOCK && passkeySupported() && (
              <button type="button" className="ui-btn auth-submit" style={{ marginTop: 8 }} onClick={() => void withPasskey()} disabled={busy}>
                비밀번호 대신 패스키로 로그인
              </button>
            )}
          </form>
          {!MOCK && <p className="auth-sub" style={{ marginTop: 12 }}>패스키는 스튜디오 계정 정보 &gt; 로그인에서 등록해요. 패스키로 들어오면 인증 앱 코드를 따로 입력하지 않아요.</p>}
        </div>
      </div>
      <p className="auth-foot">
        <span>© AUDENIQ</span>
        <span aria-hidden="true">·</span>
        <span>관리자 권한은 운영 책임자가 부여해요</span>
      </p>
    </div>
  );
}
