// 연결된 AI 앱 — AI 어시스턴트(ChatGPT·Claude 등)가 이 계정에 받은 권한을 보고, 언제든 끊는다.
// OAuth를 쓰지 못하는 에이전트에게 줄 연결 토큰도 여기서 만든다 (한 번만 보여 줌).
import { useEffect, useState } from 'react';
import { useToast } from './Toast';
import { useConfirm } from './Confirm';
import { CopyButton } from './CopyButton';
import { errorMessage } from '../api/errors';
import { orgId } from '../api/http';
import {
  aiConnections, clientHost, createConnectionToken, revokeAiConnection, SCOPE_HELP, SCOPE_LABEL,
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
/** 기간 — 0은 무기한 */
const PERIODS: { days: number; label: string }[] = [
  { days: 7, label: '7일' },
  { days: 30, label: '30일' },
  { days: 90, label: '90일' },
  { days: 0, label: '무기한' },
];

function TokenForm({ onCreated, onClose }: { onCreated: (t: NewConnectionToken) => void; onClose: () => void }) {
  const [label, setLabel] = useState('');
  const [scopes, setScopes] = useState<Record<string, boolean>>(() => Object.fromEntries(OPTIONAL_SCOPES.map(o => [o.scope, o.on])));
  const [days, setDays] = useState(30);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const all = OPTIONAL_SCOPES.every(o => scopes[o.scope]);
  const forever = days === 0;
  const strong = scopes['releases:submit'] || scopes['payouts:request'];

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (busy) return;
    setBusy(true);
    setError('');
    try {
      const picked = OPTIONAL_SCOPES.filter(o => scopes[o.scope]).map(o => o.scope);
      const period = forever ? { no_expiry: true } : { days };
      onCreated(await createConnectionToken({ label: label.trim(), org_id: orgId(), scopes: picked, ...period }));
    } catch (err) {
      setError(errorMessage(err, '연결 토큰을 만들지 못했어요.'));
    } finally {
      setBusy(false);
    }
  }

  return (
    <form className="aq-token-form" onSubmit={submit}>
      <div className="ui-field">
        <label htmlFor="aqTokenLabel">이름</label>
        <input id="aqTokenLabel" required maxLength={60} value={label} onChange={e => setLabel(e.target.value)} placeholder="예: 작업실 에이전트" autoComplete="off" autoFocus />
        <p className="ui-field-help">어떤 에이전트에 넣었는지 알아볼 수 있게 지어 주세요.</p>
      </div>

      <div className="aq-token-group" role="group" aria-labelledby="aqTokenScopes">
        <div className="aq-token-group-head">
          <p id="aqTokenScopes" className="ui-field-label">할 수 있는 일</p>
          <label className="aq-token-all">
            <input type="checkbox" checked={all} onChange={e => setScopes(Object.fromEntries(OPTIONAL_SCOPES.map(o => [o.scope, e.target.checked])))} />
            <span>모든 권한 허용</span>
          </label>
        </div>
        <label className="ui-check">
          <input type="checkbox" checked disabled />
          <span><strong>{SCOPE_LABEL['releases:read']}</strong><small>{SCOPE_HELP['releases:read']} 항상 포함돼요.</small></span>
        </label>
        {OPTIONAL_SCOPES.map(o => (
          <label key={o.scope} className="ui-check">
            <input type="checkbox" checked={scopes[o.scope]} onChange={e => setScopes(v => ({ ...v, [o.scope]: e.target.checked }))} />
            <span><strong>{SCOPE_LABEL[o.scope]}</strong><small>{SCOPE_HELP[o.scope]}</small></span>
          </label>
        ))}
      </div>

      <div className="aq-token-group">
        <p id="aqTokenPeriod" className="ui-field-label">쓸 수 있는 기간</p>
        <div className="ui-chips is-wrap" role="radiogroup" aria-labelledby="aqTokenPeriod">
          {PERIODS.map(p => (
            <button key={p.days} type="button" className="ui-chip" role="radio" aria-checked={days === p.days} aria-pressed={days === p.days} onClick={() => setDays(p.days)}>
              {p.label}
            </button>
          ))}
        </div>
      </div>

      {(forever || strong) && (
        <div className="ui-callout is-warn" role="note">
          {forever && <p>무기한 토큰은 직접 끊기 전까지 계속 쓸 수 있어요. 비밀번호를 바꾸거나 모든 기기에서 로그아웃하면 함께 끊겨요.</p>}
          {strong && <p>발매 신청·지급 신청 권한이 있으면 이 토큰을 가진 에이전트가 내 이름으로 신청할 수 있어요. 믿을 수 있는 에이전트에만 넣어 주세요.</p>}
        </div>
      )}

      {error && <p className="ui-field-help aq-help-error" role="alert">{error}</p>}
      <div className="aq-row-actions">
        <button type="button" className="ui-btn is-soft" onClick={onClose} disabled={busy}>닫기</button>
        <button type="submit" className={`ui-btn is-primary${busy ? ' is-busy' : ''}`} disabled={busy || !label.trim()} aria-busy={busy}>
          {busy ? '만드는 중' : '토큰 만들기'}
        </button>
      </div>
    </form>
  );
}

function MadeToken({ made, onClose }: { made: NewConnectionToken; onClose: () => void }) {
  return (
    <div className="ui-callout is-success aq-token-made" role="status">
      <p><strong>연결 토큰을 만들었어요.</strong></p>
      <p>이 창을 닫으면 토큰을 다시 볼 수 없어요. 지금 에이전트 설정에 넣어 주세요.</p>
      <div className="aq-token-value">
        <span className="ui-field-label">MCP 주소</span>
        <code>{made.mcp_url}</code>
        <CopyButton text={made.mcp_url} />
      </div>
      <div className="aq-token-value">
        <span className="ui-field-label">토큰 · 헤더 <code>Authorization: Bearer</code>로 보내요</span>
        <code>{made.token}</code>
        <CopyButton text={made.token} label="토큰 복사" />
      </div>
      <p className="aq-muted">
        {made.no_expiry || !made.expires_at ? '무기한으로 쓸 수 있어요.' : `${day(made.expires_at)}까지 쓸 수 있어요.`}
        {' '}토큰은 비밀번호처럼 다뤄 주세요. 대화창에 붙여 넣지 말고 에이전트 설정에만 넣어 주세요.
      </p>
      <button type="button" className="ui-btn is-soft is-sm" onClick={onClose}>닫기</button>
    </div>
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
        ChatGPT·Claude 같은 AI 앱에서 어데닉을 연결하면 여기에 나와요. 앱마다 승인한 일만 할 수 있어요.
      </p>
      {items.length === 0 ? (
        <div className="aq-token-empty">
          <strong>연결된 AI 앱이 없어요</strong>
          <small>AI 앱에서 어데닉을 연결하거나, 아래에서 연결 토큰을 만들 수 있어요.</small>
        </div>
      ) : (
        <ul className="aq-passkey-list">
          {items.map(c => {
            const host = clientHost(c.client_id);
            const used = c.last_used_at ? `마지막 사용 ${relativeTime(c.last_used_at)}` : '아직 사용 안 함';
            const token = c.method === 'TOKEN';
            const until = c.no_expiry || !c.expires_at ? '무기한' : `${day(c.expires_at)}까지`;
            const meta = token
              ? ['연결 토큰', workspaceName(c.org_name), until, used]
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

      <section className="aq-token-section" aria-labelledby="aqTokenTitle">
        <h3 id="aqTokenTitle" className="ui-field-label">연결 토큰</h3>
        {made ? (
          <MadeToken made={made} onClose={() => setMade(null)} />
        ) : making ? (
          <TokenForm onClose={() => setMaking(false)} onCreated={t => { setMade(t); setMaking(false); void load(); }} />
        ) : (
          <>
            <p className="aq-muted">OAuth 로그인을 지원하지 않는 에이전트에 넣는 토큰이에요. 지금 작업 공간에서 고른 일만 할 수 있어요.</p>
            <button type="button" className="ui-btn" onClick={() => setMaking(true)}>연결 토큰 만들기</button>
          </>
        )}
      </section>

      <p className="aq-muted aq-token-note">
        지급 계좌 변경·계약 서명·고액 지급은 어떤 앱도 할 수 없어요. 비밀번호를 바꾸거나 모든 기기에서 로그아웃하면 모든 연결이 끊겨요.
      </p>
    </div>
  );
}
