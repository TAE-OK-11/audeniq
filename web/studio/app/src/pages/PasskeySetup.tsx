// 비밀번호로 로그인·가입한 직후 한 번 — '다음부터는 비밀번호 없이 로그인하세요' (components/PasskeyOffer)
import { useEffect, useState } from 'react';
import { Navigate, useNavigate } from '../lib/router';
import { AuthLayout } from '../components/AuthLayout';
import { PasskeyOffer, shouldOfferPasskey } from '../components/PasskeyOffer';
import { useToast } from '../components/Toast';
import { clearPasskeyHandoff, peekPasskeyHandoff } from '../lib/passkeyHandoff';

export function PasskeySetup() {
  const nav = useNavigate();
  const toast = useToast();
  const [handoff] = useState(peekPasskeyHandoff);
  const [ready, setReady] = useState(false);

  useEffect(() => {
    if (!handoff) return;
    let alive = true;
    void shouldOfferPasskey().then(ok => {
      if (!alive) return;
      if (ok) setReady(true);
      else { clearPasskeyHandoff(); nav(handoff.next, { replace: true }); }
    });
    return () => { alive = false; };
  }, [handoff, nav]);

  // 새로고침 등으로 넘겨받은 값이 없으면 묻지 않고 홈으로
  if (!handoff) return <Navigate to="/" replace />;
  if (!ready) return (
    <div className="aq-boot" role="status" aria-label="불러오는 중">
      <img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="" />
      <span className="aq-boot-bar" />
    </div>
  );

  const done = (created: boolean) => {
    clearPasskeyHandoff();
    if (created) toast('패스키를 만들었어요. 다음부터 지문·얼굴·화면 잠금으로 로그인해요.', 'success');
    nav(handoff.next, { replace: true });
  };

  return (
    <AuthLayout
      icon="key" step={handoff.signup ? { at: 3, of: 3 } : undefined}
      title={handoff.signup ? '마지막으로, 패스키를 만들어요' : '다음부터는 비밀번호 없이 로그인하세요'}
      sub="이 기기에 패스키를 만들어 두면 로그인이 한 번에 끝나요."
    >
      <PasskeyOffer password={handoff.password} onDone={done} />
    </AuthLayout>
  );
}
