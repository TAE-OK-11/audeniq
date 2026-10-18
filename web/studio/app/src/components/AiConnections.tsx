// 연결된 AI 앱 — AI 어시스턴트(ChatGPT·Claude 등)가 이 계정에 받은 권한을 보고, 언제든 끊는다.
// OAuth를 쓰지 못하는 에이전트에게 줄 연결 토큰도 여기서 만든다 (한 번만 보여 줌).
import { useEffect, useState } from 'react';
import { useToast } from './Toast';
import { useConfirm } from './Confirm';
import { CopyButton } from './CopyButton';
import { errorMessage } from '../api/errors';
import { orgId } from '../api/http';
import {
  aiConnections, clientHost, createConnectionToken, revokeAiConnection, SCOPE_LABEL,
  type AiConnection, type NewConnectionToken,
} from '../api/aiConnections';
import { relativeTime } from '../lib/date';

const workspaceName = (name: string) => (name === 'Personal workspace' ? '내 작업 공간' : name);
const day = (iso: string) => new Date(iso).toLocaleDateString('ko-KR', { year: 'numeric', month: 'long', day: 'numeric' });

/** 고를 수 있는 권한 — 보기는 항상 포함. 신청·정산·지급은 기본으로 꺼 둔다. */
const OPTIONAL_SCOPES: { scope: string; on: boolean }[] = [
  { scope: 'releases:write', on: true },
  { scope: 'releases:submit', on: false },
  { scope: 'settlement:read', on: false },
  { scope: 'payouts:request', on: false },
];

function TokenForm({ onCreated, onClose }: { onCreated: (t: NewConnectionToken) => void; onClose: () => void }) {
  const [label, setLabel] = useState('');
  const [scopes, setScopes] = useState(() => Object.fromEntries(OPTIONAL_SCOPES.map(o => [o.scope, o.on])));
  const [days, setDays] = useState(30);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (busy) return;
    setBusy(true);
    setError('');
    try {
      const picked = Object.entries(scopes).filter(([, on]) => on).map(([s]) => s);
      onCreated(await createConnectionToken({ label: label.trim(), org_id: orgId(), scopes: picked, days }));
    } catch (err) {
      setError(errorMessage(err, '연결 토큰을 만들지 못했어요.'));
    } finally {
      setBusy(false);
    }
  }

  return (
    <form onSubmit={submit}>
      <div className="ui-field">
        <label htmlFor="aqTokenLabel">이름</label>
        <input id="aqTokenLabel" required maxLength={60} value={label} onChange={e => setLabel(e.target.value)} placeholder="예: 작업실 에이전트" autoComplete="off" autoFocus />
      </div>
      <fieldset className="ui-field">
        <legend>할 수 있는 일</legend>
        <label className="ui-check"><input type="checkbox" checked disabled /><span>{SCOPE_LABEL['releases:read']}</span></label>
        {OPTIONAL_SCOPES.map(o => (
          <label key={o.scope} className="ui-check">
            <input type="checkbox" checked={scopes[o.scope]} onChange={e => setScopes(v => ({ ...v, [o.scope]: e.target.checked }))} />
            <span>{SCOPE_LABEL[o.scope]}</span>
          </label>
        ))}
      </fieldset>
      <div className="ui-field">
        <label htmlFor="aqTokenDays">쓸 수 있는 기간</label>
        <select id="aqTokenDays" value={days} onChange={e => setDays(Number(e.target.value))}>
          <option value={7}>7일</option>
          <option value={30}>30일</option>
          <option value={90}>90일</option>
        </select>
      </div>
      {error && <p className="ui-field-help aq-help-error" role="alert">{error}</p>}
      <div className="aq-row-actions">
        <button type="button" className="ui-btn is-soft" onClick={onClose} disabled={busy}>닫기</button>
        <button type="submit" className={`ui-btn is-primary${busy ? ' is-busy' : ''}`} disabled={busy} aria-busy={busy}>
          {busy ? '만드는 중' : '토큰 만들기'}
        </button>
      </div>
    </form>
  );
}

export function AiConnections() {
  const toast = useToast();
  const confirm = useConfirm();
  const [items, setItems] = useState<AiConnection[] | null>(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState<string | null>(null);
  const [making, setMaking] = useState(false);
  const [made, setMade] = useState<NewConnectionToken | null>(null);

  const load = () => aiConnections().then(setItems).catch(err => setError(errorMessage(err, '연결된 AI 앱을 불러오지 못했어요.')));
  useEffect(() => { void load(); }, []);

  async function disconnect(c: AiConnection) {
    const name = c.label ?? c.client_name;
    const ok = await confirm({
      title: `${name} 연결을 끊을까요?`,
      message: c.method === 'TOKEN'
        ? '끊으면 이 토큰은 바로 쓸 수 없어요. 다시 쓰려면 연결 토큰을 새로 만들어 주세요.'
        : '끊으면 이 AI 앱은 바로 어데닉에 접근할 수 없어요. 다시 쓰려면 AI 앱에서 새로 연결하면 돼요.',
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
        ChatGPT·Claude 같은 AI 앱에서 어데닉을 연결하면 여기에 나와요. 앱마다 승인한 일만 할 수 있고,
        지급 계좌 변경·계약 서명·고액 지급은 어떤 앱도 할 수 없어요. 비밀번호를 바꾸거나 모든 기기에서 로그아웃하면 모두 끊겨요.
      </p>
      {items.length === 0 ? (
        <p>연결된 AI 앱이 없어요.</p>
      ) : (
        <ul className="aq-passkey-list">
          {items.map(c => {
            const host = clientHost(c.client_id);
            const used = c.last_used_at ? `마지막 사용 ${relativeTime(c.last_used_at)}` : '아직 사용 안 함';
            const token = c.method === 'TOKEN';
            const meta = token
              ? ['연결 토큰', workspaceName(c.org_name), c.expires_at ? `${day(c.expires_at)}까지` : null, used]
              : [host, workspaceName(c.org_name), `연결 ${relativeTime(c.created_at)}`, used];
            return (
              <li key={c.id}>
                <span className="min-0">
                  <strong>{token ? c.label ?? c.client_name : c.client_name}</strong>
                  <small>{meta.filter(Boolean).join(' · ')}</small>
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

      {made && (
        <div className="ui-callout" role="status">
          <p><strong>연결 토큰을 만들었어요.</strong> 이 창을 닫으면 다시 볼 수 없으니 지금 에이전트 설정에 넣어 주세요.</p>
          <p className="aq-muted">MCP 주소</p>
          <p><code>{made.mcp_url}</code> <CopyButton text={made.mcp_url} /></p>
          <p className="aq-muted">헤더 <code>Authorization: Bearer 토큰</code>으로 보내요.</p>
          <p><code>{made.token}</code> <CopyButton text={made.token} label="토큰 복사" /></p>
          <p className="aq-muted">{day(made.expires_at)}까지 쓸 수 있어요. 토큰은 비밀번호처럼 다뤄 주세요. 대화창에 붙여 넣지 말고 에이전트 설정에만 넣어 주세요.</p>
          <button type="button" className="ui-btn is-soft is-sm" onClick={() => setMade(null)}>닫기</button>
        </div>
      )}

      {making ? (
        <TokenForm
          onClose={() => setMaking(false)}
          onCreated={t => { setMade(t); setMaking(false); void load(); }}
        />
      ) : (
        !made && (
          <>
            <p className="aq-muted">
              OAuth 로그인을 지원하지 않는 에이전트에는 연결 토큰을 만들어 넣어 주세요. 지금 작업 공간에서 고른 일만 할 수 있어요.
            </p>
            <button type="button" className="ui-btn" onClick={() => setMaking(true)}>연결 토큰 만들기</button>
          </>
        )
      )}
    </div>
  );
}
