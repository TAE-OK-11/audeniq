// 연결된 계정 — Google·Apple로 로그인할 수 있게 지금 계정에 연결하거나 해제한다.
import { useEffect, useState } from 'react';
import { useToast } from './Toast';
import { useConfirm } from './Confirm';
import { errorMessage } from '../api/errors';
import { AppleLogo, GoogleLogo } from './SocialSignIn';
import { OAUTH_LABEL, oauthIdentities, startOAuth, unlinkOAuth, type OAuthIdentities, type OAuthProvider } from '../api/oauth';
import { SkeletonAcctRows } from './Skeleton';

export function ConnectedAccounts() {
  const toast = useToast();
  const confirm = useConfirm();
  const [data, setData] = useState<OAuthIdentities | null>(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState<OAuthProvider | null>(null);

  useEffect(() => {
    oauthIdentities().then(setData).catch(err => setError(errorMessage(err, '연결된 계정을 불러오지 못했어요.')));
  }, []);

  async function connect(p: OAuthProvider) {
    setBusy(p);
    try {
      await startOAuth(p, { mode: 'link' });
    } catch (err) {
      toast(errorMessage(err, '연결을 시작하지 못했어요.'), 'error');
      setBusy(null);
    }
  }

  async function disconnect(p: OAuthProvider, id: string) {
    const ok = await confirm({ title: `${OAUTH_LABEL[p]} 연결을 해제할까요?`, message: `해제하면 ${OAUTH_LABEL[p]}로는 로그인할 수 없어요.`, confirmLabel: '해제', danger: true });
    if (!ok) return;
    try {
      setData(await unlinkOAuth(id));
      toast('연결을 해제했어요.', 'info');
    } catch (err) {
      toast(errorMessage(err, '연결을 해제하지 못했어요.'), 'error');
    }
  }

  if (error) return <div className="ui-callout is-danger" role="alert">{error}</div>;
  if (!data) return <SkeletonAcctRows />;
  const rows = (['google', 'apple'] as const).filter(p => data.providers[p] || data.items.some(i => i.provider === p));
  if (rows.length === 0) return <p>지금은 연결할 수 있는 계정이 없어요.</p>;

  // 패스키·AI 앱 창과 같은 줄 — 회사 로고 칸 · 이름 · 연결 상태 · 오른쪽 버튼(연결은 옅은 버튼, 해제는 빨간 글자)
  return (
    <div className="aq-passkeys">
      <p className="aq-muted">연결하면 비밀번호 대신 그 계정으로 로그인할 수 있어요. 연결 정보의 이메일은 암호화해 보관해요.</p>
      <ul className="aq-acct-list">
        {rows.map(p => {
          const linked = data.items.find(i => i.provider === p);
          return (
            <li key={p}>
              <span className={`aq-acct-ico is-${p}`} aria-hidden="true">{p === 'google' ? <GoogleLogo /> : <AppleLogo />}</span>
              <span className="min-0 aq-acct-copy">
                <strong>{OAUTH_LABEL[p]}</strong>
                <small className={linked ? 'is-on' : undefined}>{linked ? `연결됨${linked.email ? ` · ${linked.email}` : ''}` : '연결 안 됨'}</small>
              </span>
              {linked
                ? <button type="button" className="ui-btn is-text is-sm aq-danger-text" onClick={() => void disconnect(p, linked.id)}>해제</button>
                : data.providers[p] && (
                  <button type="button" className="ui-btn is-soft is-sm" onClick={() => void connect(p)} disabled={busy !== null}>
                    {busy === p ? '이동하는 중' : '연결'}
                  </button>
                )}
            </li>
          );
        })}
      </ul>
      {!data.has_password && (
        <p className="aq-muted">비밀번호 없이 {data.items.map(i => OAUTH_LABEL[i.provider]).join('·')}로 가입한 계정이에요. 비밀번호가 필요하면 로그인 화면의 &lsquo;이메일 · 비밀번호 찾기&rsquo;에서 정할 수 있어요.</p>
      )}
    </div>
  );
}
