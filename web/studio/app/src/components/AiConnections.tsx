// 연결된 AI 앱 — AI 어시스턴트(ChatGPT·Claude 등)가 이 계정에 받은 권한을 보고, 언제든 끊는다.
// 에이전트마다 하나씩 주는 에이전트 키(서버의 연결 토큰)도 여기서 만든다 (한 번만 보여 줌).
// 키는 그대로 쓰거나, 토큰 교환(RFC 8693)으로 권한을 좁힌 1시간짜리 작업 토큰을 받아 쓴다.
import { useEffect, useRef, useState } from 'react';
import { useToast } from './Toast';
import { useConfirm } from './Confirm';
import { CopyButton } from './CopyButton';
import { ApiError, errorMessage } from '../api/errors';
import { orgId } from '../api/http';
import {
  aiActionRequests, aiConnections, aiPermissionRequests, aiSettings, aiSubmissions, approveAiSubmission, clientHost, createConnectionToken, DECLARATION_LABEL,
  rejectAiSubmission, revokeAiConnection, saveAiSettings, SCOPE_HELP, SCOPE_LABEL,
  type AiActionRequest, type AiConnection, type AiPermissionRequest, type AiSettings, type AiSubmission, type NewConnectionToken,
} from '../api/aiConnections';
import { Link } from '../lib/router';
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

function TokenForm({ payoutScope, onCreated, onClose }: { payoutScope: boolean; onCreated: (t: NewConnectionToken) => void; onClose: () => void }) {
  // AI 지급 한도가 0이면 서버가 지급 신청 권한을 거절하므로 고를 수 없게 한다
  const offered = OPTIONAL_SCOPES.filter(o => payoutScope || o.scope !== 'payouts:request');
  const [label, setLabel] = useState('');
  const [scopes, setScopes] = useState<Record<string, boolean>>(() => Object.fromEntries(offered.map(o => [o.scope, o.on])));
  const periodRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const [days, setDays] = useState(30);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const all = offered.every(o => scopes[o.scope]);
  const forever = days === 0;
  const strong = scopes['releases:submit'] || scopes['payouts:request'];

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (busy) return;
    const org = orgId();
    if (!org) {
      setError('작업 공간을 찾을 수 없어요. 다시 로그인해 주세요.');
      return;
    }
    setBusy(true);
    setError('');
    try {
      const picked = offered.filter(o => scopes[o.scope]).map(o => o.scope);
      const period = forever ? { no_expiry: true } : { days };
      onCreated(await createConnectionToken({ label: label.trim(), org_id: org, scopes: picked, ...period }));
    } catch (err) {
      setError(err instanceof ApiError && err.status === 404
        ? '지금은 에이전트 키를 만들 수 없어요.'
        : errorMessage(err, '에이전트 키를 만들지 못했어요.'));
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
            <input type="checkbox" checked={all} onChange={e => setScopes(Object.fromEntries(offered.map(o => [o.scope, e.target.checked])))} />
            <span>모든 권한 허용</span>
          </label>
        </div>
        <label className="ui-check">
          <input type="checkbox" checked disabled />
          <span><strong>{SCOPE_LABEL['releases:read']}</strong><small>{SCOPE_HELP['releases:read']} 항상 포함돼요.</small></span>
        </label>
        {offered.map(o => (
          <label key={o.scope} className="ui-check">
            <input type="checkbox" checked={scopes[o.scope]} onChange={e => setScopes(v => ({ ...v, [o.scope]: e.target.checked }))} />
            <span><strong>{SCOPE_LABEL[o.scope]}</strong><small>{SCOPE_HELP[o.scope]}</small></span>
          </label>
        ))}
      </div>

      <div className="aq-token-group">
        <p id="aqTokenPeriod" className="ui-field-label">쓸 수 있는 기간</p>
        <div className="ui-chips is-wrap" role="radiogroup" aria-labelledby="aqTokenPeriod">
          {PERIODS.map((p, i) => (
            <button
              key={p.days} type="button" role="radio" aria-checked={days === p.days}
              className={`ui-chip${days === p.days ? ' is-on' : ''}`}
              tabIndex={days === p.days ? 0 : -1}
              ref={el => { periodRefs.current[i] = el; }}
              onClick={() => setDays(p.days)}
              onKeyDown={e => {
                // 라디오 그룹: 화살표로 옮기면서 고른다
                const step = e.key === 'ArrowRight' || e.key === 'ArrowDown' ? 1 : e.key === 'ArrowLeft' || e.key === 'ArrowUp' ? -1 : 0;
                if (!step) return;
                e.preventDefault();
                const next = (i + step + PERIODS.length) % PERIODS.length;
                setDays(PERIODS[next].days);
                periodRefs.current[next]?.focus();
              }}
            >
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

/** 토큰 교환 예시 (POST {origin}/api/oauth/token) — 키 값은 자리표시자로 둔다 */
function exchangeExample(made: NewConnectionToken): string {
  const origin = made.mcp_url.replace(/\/api\/mcp\/?$/, '');
  return `curl -s -X POST ${origin}/api/oauth/token \\
  -d grant_type=urn:ietf:params:oauth:grant-type:token-exchange \\
  -d subject_token_type=urn:ietf:params:oauth:token-type:access_token \\
  -d subject_token=$AUDENIQ_AGENT_KEY \\
  -d scope="${made.scopes.join(' ')}"`;
}

function MadeToken({ made, onClose }: { made: NewConnectionToken; onClose: () => void }) {
  // 화면 읽기 프로그램에는 완료 문장만 알린다 (토큰 값을 소리 내어 읽지 않게)
  const title = useRef<HTMLParagraphElement>(null);
  useEffect(() => { title.current?.focus(); }, []);
  return (
    <div className="ui-callout is-success aq-token-made">
      <p role="status" tabIndex={-1} ref={title}><strong>에이전트 키를 만들었어요.</strong></p>
      <p>이 창을 닫으면 키를 다시 볼 수 없어요. 지금 에이전트 설정에 넣어 주세요.</p>
      <div className="aq-token-value">
        <span className="ui-field-label">MCP 주소</span>
        <code>{made.mcp_url}</code>
        <CopyButton text={made.mcp_url} />
      </div>
      <div className="aq-token-value">
        <span className="ui-field-label">에이전트 키 · 헤더 <code>Authorization: Bearer</code>로 보내요</span>
        <code>{made.token}</code>
        <CopyButton text={made.token} label="키 복사" />
      </div>
      <details className="aq-token-exchange">
        <summary>작업 토큰으로 바꿔 쓰기 (선택)</summary>
        <p>키를 그대로 쓰지 않고, 필요한 권한만 담은 1시간짜리 작업 토큰을 받아 쓸 수 있어요. 키에 있는 권한 안에서만 받을 수 있어요.</p>
        <pre><code>{exchangeExample(made)}</code></pre>
        <CopyButton text={exchangeExample(made)} label="예시 복사" />
      </details>
      <p className="aq-muted">
        {made.no_expiry || !made.expires_at ? '무기한으로 쓸 수 있어요.' : `${day(made.expires_at)}까지 쓸 수 있어요.`}
        {' '}키는 비밀번호처럼 다뤄 주세요. 대화창에 붙여 넣지 말고 에이전트 설정에만 넣어 주세요.
      </p>
      <button type="button" className="ui-btn is-soft is-sm" onClick={onClose}>닫기</button>
    </div>
  );
}

/** AI 앱으로 한 발매 신청 중 승인을 기다리는 것 — 승인하면 그때 접수돼요. */
function PendingSubmissions({ items, onDone }: { items: AiSubmission[]; onDone: (id: string) => void }) {
  const toast = useToast();
  const confirm = useConfirm();
  const [busy, setBusy] = useState<string | null>(null);

  async function decide(p: AiSubmission, approve: boolean) {
    const declared = Object.entries(p.declarations).filter(([, v]) => v).map(([k]) => DECLARATION_LABEL[k] ?? k);
    const ok = await confirm(approve
      ? {
        title: `‘${p.title}’ 발매를 신청할까요?`,
        message: (
          <>
            <p>{p.client_name}에서 대화로 확인한 내용 그대로 접수해요. 아래 선언은 내 이름으로 기록돼요.</p>
            <ul>{(declared.length ? declared : ['선언 없음']).map(d => <li key={d}>{d}</li>)}</ul>
          </>
        ),
        confirmLabel: '승인하고 신청하기',
      }
      : { title: `‘${p.title}’ 신청을 거절할까요?`, message: '거절하면 접수되지 않아요. 발매 초안은 그대로 남아요.', confirmLabel: '거절하기', danger: true });
    if (!ok) return;
    setBusy(p.id);
    try {
      if (approve) await approveAiSubmission(p.id); else await rejectAiSubmission(p.id);
      onDone(p.id);
      toast(approve ? '발매 신청을 접수했어요.' : '신청을 거절했어요.', approve ? 'success' : 'info');
    } catch (err) {
      toast(errorMessage(err, approve ? '신청하지 못했어요.' : '거절하지 못했어요.'), 'error');
      // 서버가 이미 정리한 신청(내용 바뀜·기한 지남·이미 처리)은 목록에서 뺀다
      if (err instanceof ApiError && ['AI_SUBMISSION_STALE', 'AI_SUBMISSION_EXPIRED', 'AI_SUBMISSION_DECIDED', 'AI_SUBMISSION_WITHDRAWN'].includes(err.code)) onDone(p.id);
    } finally {
      setBusy(null);
    }
  }

  return (
    <section className="aq-token-section" aria-labelledby="aqAiPending">
      <h3 id="aqAiPending" className="ui-field-label">승인을 기다리는 AI 신청</h3>
      <ul className="aq-passkey-list">
        {items.map(p => (
          <li key={p.id}>
            <span className="min-0">
              <strong>{p.title}</strong>
              <small>{[p.client_name, workspaceName(p.org_name), `${day(p.expires_at)}까지 승인`].join(' · ')}</small>
            </span>
            <span className="aq-row-actions">
              <button type="button" className="ui-btn is-soft is-sm" disabled={busy !== null} onClick={() => void decide(p, false)}>거절</button>
              <button type="button" className="ui-btn is-primary is-sm" disabled={busy !== null} onClick={() => void decide(p, true)}>
                {busy === p.id ? '처리 중' : '승인'}
              </button>
            </span>
          </li>
        ))}
      </ul>
    </section>
  );
}

/** AI로 하는 발매 신청을 스튜디오에서 한 번 더 승인할지 */
function StudioApproval({ initial }: { initial: boolean }) {
  const toast = useToast();
  const [on, setOn] = useState(initial);
  const [saving, setSaving] = useState(false);
  async function change(next: boolean) {
    if (saving) return;
    const before = on;
    setOn(next);
    setSaving(true);
    try {
      await saveAiSettings(next);
      toast(next ? 'AI 발매 신청은 이제 여기서 승인해야 접수돼요.' : 'AI 발매 신청이 대화에서 확인한 대로 바로 접수돼요.', 'info');
    } catch (err) {
      setOn(before);
      toast(errorMessage(err, '설정을 바꾸지 못했어요.'), 'error');
    } finally {
      setSaving(false);
    }
  }
  return (
    <label className="ui-check">
      <input type="checkbox" checked={on} disabled={saving} aria-busy={saving} onChange={e => void change(e.target.checked)} />
      <span>
        <strong>AI 발매 신청은 스튜디오에서 한 번 더 승인</strong>
        <small>켜 두면 AI 앱이 신청을 보내도 여기서 승인해야 접수돼요. 7일 안에 승인하지 않으면 취소돼요.</small>
      </span>
    </label>
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
  const [pending, setPending] = useState<AiSubmission[]>([]);
  const [settings, setSettings] = useState<AiSettings | null>(null);
  const [asks, setAsks] = useState<AiPermissionRequest[]>([]);
  const [actions, setActions] = useState<AiActionRequest[]>([]);
  useEffect(() => {
    aiSubmissions().then(setPending).catch(() => setPending([]));
    aiPermissionRequests().then(setAsks).catch(() => setAsks([]));
    aiActionRequests().then(setActions).catch(() => setActions([]));
    aiSettings().then(setSettings).catch(() => setSettings(null));
  }, []);

  useEffect(() => {
    aiConnections().then(setItems).catch(err => setError(errorMessage(err, '연결된 AI 앱을 불러오지 못했어요.')));
  }, []);
  // 토큰을 만든 뒤의 새로고침: 실패해도 화면(방금 만든 토큰)을 덮지 않는다
  const refresh = () => aiConnections().then(setItems).catch(() => toast('목록을 새로 불러오지 못했어요. 창을 다시 열어 주세요.', 'error'));

  async function disconnect(c: AiConnection) {
    const name = c.label ?? c.client_name;
    const ok = await confirm({
      title: `${name} 연결을 끊을까요?`,
      message: c.method === 'TOKEN'
        ? '끊으면 이 키와 키로 받은 작업 토큰을 바로 쓸 수 없어요. 다시 쓰려면 에이전트 키를 새로 만들어 주세요.'
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
      {asks.length > 0 && (
        <section className="aq-token-section" aria-labelledby="aqAiAsks">
          <h3 id="aqAiAsks" className="ui-field-label">권한 추가 요청</h3>
          <ul className="aq-passkey-list">
            {asks.map(q => (
              <li key={q.id}>
                <span className="min-0">
                  <strong>{q.client_name}</strong>
                  <small>{[q.scopes.map(s => SCOPE_LABEL[s] ?? s).join('·'), workspaceName(q.org_name)].join(' · ')}</small>
                </span>
                <span className="aq-row-actions"><Link className="ui-btn is-primary is-sm" to={`/ai/approve/${q.id}`}>확인하기</Link></span>
              </li>
            ))}
          </ul>
        </section>
      )}
      {actions.length > 0 && (
        <section className="aq-token-section" aria-labelledby="aqAiActions">
          <h3 id="aqAiActions" className="ui-field-label">승인을 기다리는 작업</h3>
          <ul className="aq-passkey-list">
            {actions.map(q => (
              <li key={q.id}>
                <span className="min-0">
                  <strong>{q.summary.title}</strong>
                  <small>{[q.client_name, workspaceName(q.org_name)].join(' · ')}</small>
                </span>
                <span className="aq-row-actions"><Link className="ui-btn is-primary is-sm" to={`/ai/actions/${q.id}`}>확인하기</Link></span>
              </li>
            ))}
          </ul>
        </section>
      )}
      {pending.length > 0 && <PendingSubmissions items={pending} onDone={id => setPending(list => list.filter(p => p.id !== id))} />}
      <p className="aq-muted">
        ChatGPT·Claude 같은 AI 앱에서 어데닉을 연결하면 여기에 나와요. 앱마다 승인한 일만 할 수 있어요.
      </p>
      {items.length === 0 ? (
        <div className="aq-token-empty">
          <strong>연결된 AI 앱이 없어요</strong>
          <small>AI 앱에서 어데닉을 연결하거나, 아래에서 에이전트 키를 만들 수 있어요.</small>
        </div>
      ) : (
        <ul className="aq-passkey-list">
          {items.map(c => {
            const host = clientHost(c.client_id);
            const used = c.last_used_at ? `마지막 사용 ${relativeTime(c.last_used_at)}` : '아직 사용 안 함';
            const token = c.method === 'TOKEN';
            const until = c.no_expiry || !c.expires_at ? '무기한' : `${day(c.expires_at)}까지`;
            const meta = token
              ? ['에이전트 키', workspaceName(c.org_name), until, used]
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

      {(settings?.mcp_enabled !== false || made) && (
      <section className="aq-token-section" aria-labelledby="aqTokenTitle">
        <h3 id="aqTokenTitle" className="ui-field-label">에이전트 키</h3>
        {made ? (
          <MadeToken made={made} onClose={() => setMade(null)} />
        ) : making ? (
          <TokenForm payoutScope={settings?.payout_scope !== false} onClose={() => setMaking(false)} onCreated={t => { setMade(t); setMaking(false); void refresh(); }} />
        ) : (
          <>
            <p className="aq-muted">에이전트마다 하나씩 만들어 주세요. 키를 그대로 쓰거나, 키로 권한을 좁힌 1시간짜리 작업 토큰을 받아 쓸 수 있어요. 지금 작업 공간에서 고른 일만 할 수 있고, 더 필요하면 에이전트가 권한 추가를 요청해요.</p>
            <button type="button" className="ui-btn" onClick={() => setMaking(true)}>에이전트 키 만들기</button>
          </>
        )}
      </section>
      )}

      {settings && (
        <section className="aq-token-section aq-token-group" aria-label="AI 신청 설정">
          <StudioApproval initial={settings.studio_approval} />
        </section>
      )}

      <p className="aq-muted aq-token-note">
        지급 계좌 변경·계약 서명·고액 지급은 어떤 앱도 할 수 없어요. 비밀번호를 바꾸거나 모든 기기에서 로그아웃하면 모든 연결이 끊겨요.
      </p>
    </div>
  );
}
