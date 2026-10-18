// 연결된 AI 앱 — AI 어시스턴트(ChatGPT·Claude 등)가 이 계정에 받은 권한을 보고, 언제든 끊는다.
import { useEffect, useState } from 'react';
import { useToast } from './Toast';
import { useConfirm } from './Confirm';
import { errorMessage } from '../api/errors';
import { aiConnections, clientHost, revokeAiConnection, SCOPE_LABEL, type AiConnection } from '../api/aiConnections';
import { relativeTime } from '../lib/date';

const workspaceName = (name: string) => (name === 'Personal workspace' ? '내 작업 공간' : name);

export function AiConnections() {
  const toast = useToast();
  const confirm = useConfirm();
  const [items, setItems] = useState<AiConnection[] | null>(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState<string | null>(null);

  useEffect(() => {
    aiConnections().then(setItems).catch(err => setError(errorMessage(err, '연결된 AI 앱을 불러오지 못했어요.')));
  }, []);

  async function disconnect(c: AiConnection) {
    const ok = await confirm({
      title: `${c.client_name} 연결을 끊을까요?`,
      message: '끊으면 이 AI 앱은 바로 AUDENIQ에 접근할 수 없어요. 다시 쓰려면 AI 앱에서 새로 연결하면 돼요.',
      confirmLabel: '연결 끊기',
      danger: true,
    });
    if (!ok) return;
    setBusy(c.id);
    try {
      await revokeAiConnection(c.id);
      setItems(list => (list ?? []).filter(i => i.id !== c.id));
      toast('연결을 끊었어요.', 'info');
    } catch (err) {
      toast(errorMessage(err, '연결을 끊지 못했어요.'), 'error');
    } finally {
      setBusy(null);
    }
  }

  if (error) return <div className="ui-callout is-danger" role="alert">{error}</div>;
  if (!items) return <p role="status">불러오는 중이에요.</p>;

  return (
    <div className="aq-passkeys">
      <p className="aq-muted">
        ChatGPT·Claude 같은 AI 앱에서 AUDENIQ를 연결하면 여기에 나와요. 앱마다 승인한 일만 할 수 있고,
        지급 계좌 변경·계약 서명·고액 지급은 어떤 앱도 할 수 없어요. 비밀번호를 바꾸거나 모든 기기에서 로그아웃하면 모두 끊겨요.
      </p>
      {items.length === 0 ? (
        <p>연결된 AI 앱이 없어요.</p>
      ) : (
        <ul className="aq-passkey-list">
          {items.map(c => {
            const host = clientHost(c.client_id);
            const used = c.last_used_at ? `마지막 사용 ${relativeTime(c.last_used_at)}` : '아직 사용 안 함';
            return (
              <li key={c.id}>
                <span className="min-0">
                  <strong>{c.client_name}</strong>
                  <small>{[host, workspaceName(c.org_name), `연결 ${relativeTime(c.created_at)}`, used].filter(Boolean).join(' · ')}</small>
                  <small>{c.scopes.map(s => SCOPE_LABEL[s] ?? s).join(', ')}</small>
                </span>
                <button type="button" className="ui-btn is-soft is-sm" onClick={() => void disconnect(c)} disabled={busy !== null}>
                  {busy === c.id ? '끊는 중' : '연결 끊기'}
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
