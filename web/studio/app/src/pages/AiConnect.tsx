// AI 앱 연결 (/ai/connect) — ChatGPT·Claude·Grok 같은 AI 앱이 어데닉을 쓰겠다고 할 때 아티스트가 보는 화면.
//  - ?request=…  : 서버가 확인하고 서명한 연결 요청 (브라우저 연결 또는 기기 코드 연결)
//  - ?user_code= : AI 앱이 알려 준 코드로 온 기기 연결 → 코드를 확인해 요청으로 바꾼다
//  - ?device=1   : 코드를 직접 입력
// 누가(어느 앱이, 어디서) 무엇을 하려는지 → 어느 작업 공간에 → 무엇을 허용할지 → 언제나 지켜지는 것 순서로 보여 주고,
// 결정은 화면 아래 고정 막대에서 한다. 승인하면 브라우저 연결은 AI 앱으로 돌아가고, 기기 연결은 여기서 끝난다.
import { useEffect, useMemo, useRef, useState } from 'react';
import { Link, useLocation } from '../lib/router';
import { useAuth } from '../api/auth';
import { ApiError, errorMessage } from '../api/errors';
import { aiConnectByCode, aiConnectDetails, decideAiConnect, type AiConnectDetails, type AiConnectScope } from '../api/aiConnect';
import { Glyph } from '../components/Glyph';
import { MOCK } from '../lib/mode';
import '../styles/ai-connect.css';

type Phase =
  | { at: 'code'; error?: string }
  | { at: 'loading' }
  | { at: 'review'; request: string; d: AiConnectDetails }
  | { at: 'done'; approved: boolean; d: AiConnectDetails; redirect?: string; granted: string[] }
  | { at: 'failed'; code: string; message: string };

const ROLE: Record<string, string> = { OWNER: '소유자', EDITOR: '편집자', VIEWER: '보기 전용' };
const workspaceName = (name: string) => (name === 'Personal workspace' ? '내 작업 공간' : name);

/** 앱마다 알아보기 쉬운 색 (공식 로고는 쓰지 않는다) */
function appTone(name: string): string {
  const n = name.toLowerCase();
  if (n.includes('claude')) return '#D97757';
  if (n.includes('chatgpt') || n.includes('openai')) return '#10A37F';
  if (n.includes('grok')) return '#171B29';
  if (n.includes('gemini')) return '#4F7DF3';
  if (n.includes('manus')) return '#5B5BD6';
  let h = 0;
  for (const c of n) h = (h * 31 + c.charCodeAt(0)) % 360;
  return `hsl(${h} 55% 45%)`;
}
const initial = (name: string) => (name.match(/[\p{L}\p{N}]/u)?.[0] ?? 'A').toUpperCase();

const SCOPE_ICON: Record<string, string> = {
  'releases:read': 'disc', 'releases:write': 'pencil', 'releases:submit': 'release',
  'settlement:read': 'chart', 'payouts:request': 'won',
};

/** 고르기 쉬운 묶음 — 대부분은 '초안까지'면 충분하다 */
const PRESETS: { id: string; label: string; scopes: string[] }[] = [
  { id: 'read', label: '보기만', scopes: [] },
  { id: 'draft', label: '초안 작성까지', scopes: ['releases:write'] },
  { id: 'all', label: '모두', scopes: ['releases:write', 'releases:submit', 'settlement:read', 'payouts:request'] },
];

const FAILED_TEXT: Record<string, string> = {
  AI_CONNECT_EXPIRED: '연결 요청 시간(10분)이 지났어요.',
  AI_CONNECT_OTHER_SESSION: '연결을 시작한 뒤 로그인이 바뀌었어요.',
  AI_CONNECT_CLIENT_UNKNOWN: '등록되지 않았거나 사용이 중단된 AI 앱이에요.',
};

const demo: AiConnectDetails = {
  kind: 'browser',
  client: { name: 'Claude', client_id: 'https://claude.ai/oauth/mcp-oauth-client-metadata', verified_host: 'claude.ai', registered: false },
  requested_from: 'claude.ai', return_to: 'claude.ai', device_code: null,
  scopes: [
    { scope: 'releases:read', title: '발매 정보 보기', description: '발매·트랙·검사 결과·접수 상태를 읽어요.', locked: true, checked: true, risk: 'none' },
    { scope: 'releases:write', title: '발매 초안 작성·수정', description: '임시 저장 상태의 발매 정보·트랙·크레딧을 만들고 고쳐요. 접수된 발매는 바꾸지 못해요.', locked: false, checked: true, risk: 'none' },
    { scope: 'releases:submit', title: '발매 신청 접수', description: '내가 대화에서 최종 확인한 내용 그대로 배급 신청을 접수해요.', locked: false, checked: false, risk: 'submit' },
    { scope: 'settlement:read', title: '정산 보기', description: '정산 잔액·내역·플랫폼별 수익 보고를 읽어요. 계좌번호는 보이지 않아요.', locked: false, checked: false, risk: 'none' },
    { scope: 'payouts:request', title: '소액 지급 신청', description: '등록해 둔 계좌로 300,000원 이하의 지급 신청과 취소를 해요.', locked: false, checked: false, risk: 'money' },
  ],
  workspaces: [{ id: 'o1', name: 'Personal workspace', role: 'OWNER' }, { id: 'o2', name: 'Han River Label', role: 'EDITOR' }],
  expires_at: new Date(Date.now() + 600_000).toISOString(),
};

export function AiConnect() {
  const loc = useLocation();
  const params = useMemo(() => new URLSearchParams(loc.search), [loc.search]);
  const [phase, setPhase] = useState<Phase>({ at: 'loading' });

  const open = (request: string) => {
    setPhase({ at: 'loading' });
    if (MOCK) { setPhase({ at: 'review', request, d: demo }); return; }
    aiConnectDetails(request).then(
      d => setPhase({ at: 'review', request, d }),
      err => setPhase(failed(err)),
    );
  };
  const byCode = (code: string) => {
    setPhase({ at: 'loading' });
    aiConnectByCode(code).then(
      r => open(r.request),
      err => setPhase({ at: 'code', error: errorMessage(err, '코드를 확인하지 못했어요. 다시 입력해 주세요.') }),
    );
  };

  useEffect(() => {
    const request = params.get('request');
    const code = params.get('user_code');
    if (request) open(request);
    else if (code) byCode(code);
    else setPhase({ at: 'code' });
    // 주소가 바뀔 때만 다시 읽는다
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [params]);

  return (
    <main className="aq-connect">
      <div className="aq-connect-glow" aria-hidden="true" />
      <TopBar />
      {phase.at === 'loading' && <div className="aq-connect-body"><p className="aq-connect-loading" role="status">연결 요청을 확인하고 있어요.</p></div>}
      {phase.at === 'code' && <CodeEntry error={phase.error} onSubmit={byCode} />}
      {phase.at === 'review' && (
        <Review
          request={phase.request} d={phase.d}
          onDone={(approved, granted, redirect) => setPhase({ at: 'done', approved, d: phase.d, granted, redirect })}
          onFailed={err => setPhase(failed(err))}
        />
      )}
      {phase.at === 'done' && <Done {...phase} />}
      {phase.at === 'failed' && <Failed code={phase.code} message={phase.message} />}
    </main>
  );
}

function failed(err: unknown): Phase {
  const code = err instanceof ApiError ? err.code : '';
  return { at: 'failed', code, message: FAILED_TEXT[code] ?? errorMessage(err, '연결 요청을 열지 못했어요.') };
}

function TopBar() {
  const { user } = useAuth();
  return (
    <header className="aq-connect-top">
      <span className="aq-connect-brand">
        <img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" />
        <span className="ui-product">STUDIO</span>
      </span>
      {user?.email && (
        <span className="aq-connect-account" title="이 계정으로 연결돼요">
          <Glyph name="user" size={14} />
          <span className="min-0">{user.email}</span>
        </span>
      )}
    </header>
  );
}

/** 앱 ↔ 어데닉 — 무엇과 무엇이 이어지는지 한눈에 */
function Handshake({ name }: { name: string }) {
  return (
    <div className="aq-connect-handshake" aria-hidden="true">
      <span className="aq-connect-app" style={{ '--app': appTone(name) } as React.CSSProperties}>{initial(name)}</span>
      <span className="aq-connect-wire"><span className="aq-connect-lock"><Glyph name="lock" size={14} /></span></span>
      <span className="aq-connect-app is-audeniq"><img src={`${import.meta.env.BASE_URL}static/favicon.svg`} alt="" /></span>
    </div>
  );
}

function Origin({ d }: { d: AiConnectDetails }) {
  if (d.client.verified_host) {
    return <span className="aq-connect-origin is-verified"><Glyph name="check" size={13} />{d.client.verified_host}에서 확인된 앱</span>;
  }
  if (d.client.registered) {
    return <span className="aq-connect-origin is-verified"><Glyph name="check" size={13} />어데닉에 등록된 에이전트</span>;
  }
  return <span className="aq-connect-origin is-unknown"><Glyph name="warning" size={13} />{d.requested_from ? `${d.requested_from}에서 요청` : '확인되지 않은 앱'}</span>;
}

function useCountdown(until: string): string {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(t);
  }, []);
  const left = Math.max(0, Math.floor((new Date(until).getTime() - now) / 1000));
  return left === 0 ? '시간이 지났어요' : `${Math.floor(left / 60)}:${String(left % 60).padStart(2, '0')} 남았어요`;
}

function Review({ request, d, onDone, onFailed }: {
  request: string; d: AiConnectDetails;
  onDone: (approved: boolean, granted: string[], redirect?: string) => void;
  onFailed: (err: unknown) => void;
}) {
  const editable = d.workspaces.filter(w => w.role !== 'VIEWER');
  const [org, setOrg] = useState(() => (editable[0] ?? d.workspaces[0])?.id ?? '');
  const [on, setOn] = useState<Set<string>>(() => new Set(d.scopes.filter(s => s.checked).map(s => s.scope)));
  const [busy, setBusy] = useState<'approve' | 'deny' | null>(null);
  const [confirming, setConfirming] = useState(false);
  const left = useCountdown(d.expires_at);
  const viewer = d.workspaces.find(w => w.id === org)?.role === 'VIEWER';
  const offered = new Set(d.scopes.map(s => s.scope));
  const preset = PRESETS.find(p => {
    const want = new Set(['releases:read', ...p.scopes.filter(s => offered.has(s))]);
    return want.size === on.size && [...want].every(s => on.has(s));
  })?.id;
  const risky = d.scopes.filter(s => s.risk !== 'none' && on.has(s.scope));

  const toggle = (s: AiConnectScope) => {
    if (s.locked) return;
    setOn(prev => {
      const next = new Set(prev);
      if (next.has(s.scope)) next.delete(s.scope); else next.add(s.scope);
      return next;
    });
    setConfirming(false);
  };
  const pick = (id: string) => {
    const p = PRESETS.find(x => x.id === id);
    if (!p) return;
    setOn(new Set(['releases:read', ...p.scopes.filter(s => offered.has(s))]));
    setConfirming(false);
  };

  async function decide(approve: boolean) {
    if (busy) return;
    // 돈이 움직이거나 내 이름으로 접수되는 권한은 한 번 더 확인한다 (같은 막대 안에서)
    if (approve && risky.length && !confirming) { setConfirming(true); return; }
    setBusy(approve ? 'approve' : 'deny');
    try {
      const res = MOCK
        ? { kind: d.kind, status: approve ? ('APPROVED' as const) : ('DENIED' as const), scopes: [...on] }
        : await decideAiConnect({ request, decision: approve ? 'approve' : 'deny', org, scopes: [...on] });
      onDone(res.status === 'APPROVED', res.scopes, 'redirect' in res ? res.redirect : undefined);
    } catch (err) {
      onFailed(err);
    } finally {
      setBusy(null);
    }
  }

  return (
    <>
      <div className="aq-connect-body">
        <section className="aq-connect-hero">
          <Handshake name={d.client.name} />
          <Origin d={d} />
          <h1><b>{d.client.name}</b>에서 어데닉을 쓸 수 있게 할까요?</h1>
          <p>허용한 일만 할 수 있고, 연결은 언제든 끊을 수 있어요.</p>
        </section>

        {d.device_code && (
          <section className="aq-connect-code" aria-label="연결 코드">
            <span>AI 앱에 보이는 코드와 같은지 확인해 주세요</span>
            <strong>{d.device_code}</strong>
          </section>
        )}
        {!d.client.verified_host && !d.client.registered && (
          <div className="ui-callout is-warn aq-connect-note">
            앱이 누구인지 어데닉이 확인하지 못했어요. 내가 방금 시작한 연결일 때만 허용해 주세요.
          </div>
        )}

        <section className="aq-connect-section" aria-labelledby="aqConnectWs">
          <h2 id="aqConnectWs"><span className="aq-connect-step">1</span>어느 작업 공간에 연결할까요?</h2>
          <div className="aq-connect-workspaces" role="radiogroup" aria-labelledby="aqConnectWs">
            {d.workspaces.map(w => (
              <button
                key={w.id} type="button" role="radio" aria-checked={org === w.id}
                className={`aq-connect-ws${org === w.id ? ' is-on' : ''}`} onClick={() => setOrg(w.id)}
              >
                <span className="aq-connect-ws-mark">{initial(workspaceName(w.name))}</span>
                <span className="min-0"><b>{workspaceName(w.name)}</b><small>{ROLE[w.role] ?? w.role}</small></span>
                <span className="aq-connect-radio" aria-hidden="true" />
              </button>
            ))}
          </div>
          {viewer && <p className="aq-connect-hint">이 작업 공간에서는 보기 전용이라, 허용해도 읽기만 할 수 있어요.</p>}
        </section>

        <section className="aq-connect-section" aria-labelledby="aqConnectScopes">
          <div className="aq-connect-section-head">
            <h2 id="aqConnectScopes"><span className="aq-connect-step">2</span>무엇을 허용할까요?</h2>
            <div className="aq-connect-presets" role="group" aria-label="빠르게 고르기">
              {PRESETS.filter(p => p.id !== 'all' || d.scopes.length > 2).map(p => (
                <button key={p.id} type="button" className={preset === p.id ? 'is-on' : undefined} aria-pressed={preset === p.id} onClick={() => pick(p.id)}>
                  {p.label}{p.id === 'draft' && <em>추천</em>}
                </button>
              ))}
            </div>
          </div>
          <ul className="aq-connect-scopes">
            {d.scopes.map(s => {
              const checked = on.has(s.scope);
              return (
                <li key={s.scope}>
                  <label className={`aq-connect-scope${checked ? ' is-on' : ''}${s.locked ? ' is-locked' : ''}`}>
                    <span className={`aq-connect-scope-icon is-${s.risk}`}><Glyph name={SCOPE_ICON[s.scope] ?? 'dot'} size={18} /></span>
                    <span className="min-0">
                      <b>
                        {s.title}
                        {s.locked && <em className="is-base">항상 포함</em>}
                        {s.risk === 'submit' && <em className="is-warn">내 이름으로 접수</em>}
                        {s.risk === 'money' && <em className="is-warn">돈이 움직여요</em>}
                      </b>
                      <small>{s.description}</small>
                    </span>
                    {/* 스튜디오 공통 스위치 (live.css .aq-switch) */}
                    <span className="aq-switch aq-connect-switch">
                      <input type="checkbox" role="switch" checked={checked} disabled={s.locked} onChange={() => toggle(s)} aria-label={s.title} />
                      <span />
                    </span>
                  </label>
                </li>
              );
            })}
          </ul>
        </section>

        <section className="aq-connect-section aq-connect-promise" aria-labelledby="aqConnectNever">
          <h2 id="aqConnectNever"><span className="aq-connect-step is-shield"><Glyph name="lock" size={13} /></span>어떤 권한을 줘도 지켜져요</h2>
          <ul>
            <li><Glyph name="card" size={16} /><span>지급 계좌를 바꾸거나 계약서·신청서에 서명할 수 없어요.</span></li>
            <li><Glyph name="approval" size={16} /><span>발매 취소·서비스 중단·큰 금액 지급은 요청만 할 수 있어요. 매번 스튜디오에서 내가 승인해요.</span></li>
            <li><Glyph name="user" size={16} /><span>비밀번호·로그인 같은 계정 설정은 건드릴 수 없어요.</span></li>
            <li><Glyph name="close" size={16} /><span>프로필 → 연결된 AI 앱에서 언제든 끊을 수 있어요.</span></li>
          </ul>
        </section>

        <dl className="aq-connect-facts">
          {d.return_to && <><dt>승인 후 돌아갈 곳</dt><dd>{d.return_to}</dd></>}
          {d.client.client_id.startsWith('https://') && <><dt>앱 확인 주소</dt><dd><code>{d.client.client_id}</code></dd></>}
          <dt>요청 시간</dt><dd>{left}</dd>
        </dl>
      </div>

      <div className={`aq-connect-bar${confirming ? ' is-confirming' : ''}`}>
        {confirming ? (
          <p className="aq-connect-bar-note" role="alert">
            <b>{risky.map(s => s.title).join('·')}</b> 권한도 줄까요? {risky.some(s => s.risk === 'money') ? '지급 신청은 대화에서 금액을 확인한 뒤에만 해요.' : '접수는 대화에서 내용을 확인한 뒤에만 해요.'}
          </p>
        ) : (
          <p className="aq-connect-bar-note">
            <b>{workspaceName(d.workspaces.find(w => w.id === org)?.name ?? '')}</b>에 권한 {on.size}개
          </p>
        )}
        <div className="aq-connect-bar-actions">
          <button type="button" className="ui-btn is-soft is-lg" disabled={!!busy} onClick={() => (confirming ? setConfirming(false) : void decide(false))}>
            {confirming ? '다시 고를게요' : '거절'}
          </button>
          <button type="button" className={`ui-btn is-primary is-lg${busy === 'approve' ? ' is-busy' : ''}`} disabled={!!busy || !org} onClick={() => void decide(true)}>
            {confirming ? '네, 허용하고 연결' : '허용하고 연결'}
          </button>
        </div>
      </div>
    </>
  );
}

function Done({ approved, d, redirect, granted }: { approved: boolean; d: AiConnectDetails; redirect?: string; granted: string[] }) {
  const titles = d.scopes.filter(s => granted.includes(s.scope)).map(s => s.title);
  useEffect(() => {
    if (!redirect) return;
    // 결과를 잠깐 보여 준 뒤 AI 앱으로 (같은 창 — 앱 안 브라우저는 새 창을 못 연다)
    const t = window.setTimeout(() => window.location.assign(redirect), approved ? 1400 : 600);
    return () => window.clearTimeout(t);
  }, [redirect, approved]);
  return (
    <div className="aq-connect-body aq-connect-result">
      <span className={`aq-connect-result-mark${approved ? ' is-ok' : ''}`} aria-hidden="true">
        <Glyph name={approved ? 'check' : 'close'} size={30} />
      </span>
      <h1>{approved ? '연결했어요' : '연결하지 않았어요'}</h1>
      {approved ? (
        <>
          <p>{redirect ? `${d.client.name} 대화로 돌아가고 있어요.` : `${d.client.name} 대화로 돌아가 이어 가 주세요.`}</p>
          <ul className="aq-connect-granted">{titles.map(t => <li key={t}><Glyph name="check" size={14} />{t}</li>)}</ul>
        </>
      ) : (
        <p>이 앱은 어데닉 계정에 접근할 수 없어요.{redirect ? ' AI 앱으로 돌아가고 있어요.' : ''}</p>
      )}
      <div className="aq-connect-result-actions">
        {redirect && <a className="ui-btn is-primary is-lg" href={redirect}>지금 돌아가기</a>}
        <Link className="ui-btn is-soft is-lg" to="/profile?open=ai">연결된 AI 앱 보기</Link>
      </div>
    </div>
  );
}

function Failed({ code, message }: { code: string; message: string }) {
  return (
    <div className="aq-connect-body aq-connect-result">
      <span className="aq-connect-result-mark is-bad" aria-hidden="true"><Glyph name="alert" size={30} /></span>
      <h1>연결 요청을 열 수 없어요</h1>
      <p>{message}</p>
      {code !== 'AI_CONNECT_CLIENT_UNKNOWN' && <p className="aq-connect-hint">AI 앱에서 연결을 처음부터 다시 시작해 주세요. 새 링크로 이 화면이 다시 열려요.</p>}
      <div className="aq-connect-result-actions">
        <Link className="ui-btn is-soft is-lg" to="/">스튜디오 홈</Link>
      </div>
    </div>
  );
}

/** 기기 연결 — AI 앱이 알려 준 8자리 코드 */
function CodeEntry({ error, onSubmit }: { error?: string; onSubmit: (code: string) => void }) {
  const [value, setValue] = useState('');
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => { ref.current?.focus(); }, []);
  const clean = value.toUpperCase().replace(/[^A-Z]/g, '').slice(0, 8);
  const shown = clean.length > 4 ? `${clean.slice(0, 4)}-${clean.slice(4)}` : clean;
  return (
    <div className="aq-connect-body">
      <section className="aq-connect-hero">
        <div className="aq-connect-handshake is-single" aria-hidden="true">
          <span className="aq-connect-app is-audeniq"><img src={`${import.meta.env.BASE_URL}static/favicon.svg`} alt="" /></span>
        </div>
        <h1>AI 앱에 보이는 연결 코드를 입력해 주세요</h1>
        <p>코드는 AI 앱이 대화에서 알려 줘요. 영문 8자리예요.</p>
      </section>
      <form className="aq-connect-code-form" onSubmit={e => { e.preventDefault(); if (clean.length === 8) onSubmit(clean); }}>
        <input
          ref={ref} className="aq-connect-code-input" value={shown} inputMode="text" autoComplete="one-time-code"
          autoCapitalize="characters" spellCheck={false} placeholder="BCDF-GHJK" aria-label="연결 코드"
          aria-invalid={!!error} onChange={e => setValue(e.target.value)}
        />
        {error && <p className="aq-connect-error" role="alert">{error}</p>}
        <button type="submit" className="ui-btn is-primary is-lg" disabled={clean.length !== 8}>다음</button>
        <p className="aq-connect-hint">내가 시작하지 않은 연결이라면 입력하지 마세요.</p>
      </form>
    </div>
  );
}
