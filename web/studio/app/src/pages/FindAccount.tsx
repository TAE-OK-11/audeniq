import { useState } from 'react';
import { Link, useNavigate } from '../lib/router';
import { AuthLayout } from '../components/AuthLayout';
import { SUPPORT_EMAIL } from '../lib/support';
import { Segmented } from '../components/Segmented';
import { Glyph } from '../components/Glyph';
import { useAuth } from '../api/auth';
import { MOCK } from '../api/client';
import { passkeySupported } from '../api/passkey';
import { ApiError, authErrorMessage } from '../api/errors';

// 계정 복구 API와 메일 발송이 연결되기 전에는 입력을 수집하거나 발송을 약속하지 않는다.
const supportMail = (tab: 'id' | 'password') => {
  const subject = tab === 'id' ? '가입한 이메일 찾기 요청' : '비밀번호 재설정 요청';
  const body = tab === 'id'
    ? '활동명:\n가입할 때 쓴 것 같은 이메일(아는 만큼):\n연락받을 이메일:\n'
    : '가입한 이메일:\n활동명:\n';
  return `mailto:${SUPPORT_EMAIL}?subject=${encodeURIComponent(subject)}&body=${encodeURIComponent(body)}`;
};

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
    <AuthLayout icon="info" title="계정을 찾고 있나요?" sub="계정 찾기와 비밀번호 재설정은 고객지원에서 안내해 드려요.">
      {error && <div className="ui-callout is-danger aq-shake" role="alert" key={error}>{error}</div>}
      <Segmented
        tabs className="aq-seg-tabs" label="계정 찾기 구분" value={tab} onChange={setTab}
        options={[{ value: 'id', label: '이메일 찾기' }, { value: 'password', label: '비밀번호 찾기' }] as const}
      />
      <div className="aq-find-panel" role="tabpanel">
        <p className="aq-muted">{tab === 'id' ? '가입한 이메일이 기억나지 않으면' : '비밀번호를 재설정해야 한다면'} 고객지원에 문의해 주세요. 아직은 자동으로 찾아 드리지 못해요.</p>
        {/* 메일 제목·본문을 미리 채워 두어 무엇을 적어야 할지 고민하지 않게 */}
        <a href={supportMail(tab)} className="ui-btn is-primary auth-submit">고객지원에 이메일 보내기</a>
      </div>
      {/* 이 기기에 패스키가 있으면 이메일·비밀번호 없이 바로 — 고객지원 아래 '또는'으로 조용히 */}
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
