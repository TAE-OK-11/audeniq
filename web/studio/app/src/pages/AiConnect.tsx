// AI 앱 연결 (/ai/connect) — ChatGPT·Claude·Grok 같은 AI 앱이 어데닉을 쓰겠다고 할 때 아티스트가 보는 화면.
//  - ?request=…  : 서버가 확인하고 서명한 연결 요청 (브라우저 연결 또는 기기 코드 연결)
//  - ?user_code= : AI 앱이 알려 준 코드로 온 기기 연결 → 코드를 확인해 요청으로 바꾼다
//  - ?device=1   : 코드를 직접 입력
// 발매 신청서(pages/Upload.tsx)와 같은 틀로 한 화면에 보여 준다: 위에 고정된 제목 막대, 신청서의 단계 제목,
// 고르는 카드(aq-choice)·옵션 카드(aq-option)·칩(ui-chip), 아래 고정 버튼(step-actions).
// 위에서부터 누가(어느 앱이, 어디서) → 어느 작업 공간에 → 무엇을 허용할지 → 어떤 권한을 줘도 지켜지는 것.
// 돈·접수 권한과 거절은 확인 창으로 한 번 더 묻는다.
// 승인하면 브라우저 연결은 AI 앱으로 돌아가고, 기기 연결은 여기서 끝난다.
import { Fragment, useEffect, useMemo, useState } from 'react';
import { Link, useLocation } from '../lib/router';
import { useAuth } from '../api/auth';
import { ApiError, errorMessage } from '../api/errors';
import { aiConnectByCode, aiConnectDetails, decideAiConnect, type AiConnectDetails } from '../api/aiConnect';
import { useConfirm } from '../components/Confirm';
import { MOCK } from '../lib/mode';
import '../styles/ai-connect.css';

type Phase =
  | { at: 'code'; error?: string }
  | { at: 'loading' }
  | { at: 'review'; request: string; d: AiConnectDetails }
  | { at: 'done'; approved: boolean; d: AiConnectDetails; redirect?: string; granted: string[] }
  | { at: 'failed'; code: string; message: string };

const ROLE: Record<string, string> = { OWNER: '소유자', EDITOR: '편집자', VIEWER: '보기 전용 — 연결해도 읽기만 할 수 있어요' };
const workspaceName = (name: string) => (name === 'Personal workspace' ? '내 작업 공간' : name);

/** 고르기 쉬운 묶음 — 대부분은 '초안 작성까지'면 충분하다 */
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

  // 신청서와 같은 화면 모드 (흰 바탕, 아래 고정 버튼 자리)
  useEffect(() => {
    document.body.classList.add('wizard-mode');
    return () => document.body.classList.remove('wizard-mode');
  }, []);

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
    <div className="portal-layout aq-connect-layout">
    <section id="view-new" className="view aq-connect-view">
      <div className="wizard">
        {phase.at === 'loading' && (
          <Frame step={-1}>
            <div className="ui-callout aq-loading-notice" role="status"><span className="aq-spinner" aria-hidden="true" /> 연결 요청을 확인하고 있어요…</div>
          </Frame>
        )}
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
      </div>
    </section>
    </div>
  );
}

function failed(err: unknown): Phase {
  const code = err instanceof ApiError ? err.code : '';
  return { at: 'failed', code, message: FAILED_TEXT[code] ?? errorMessage(err, '연결 요청을 열지 못했어요.') };
}

function BackIcon() {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" width="24" height="24" fill="none"
      stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="m15 18-6-6 6-6" />
    </svg>
  );
}

/** 신청서와 같은 위 막대(제목·로그인 계정). 연결은 한 화면에서 끝나 진행 막대는 없다 */
function Frame({ onBack, children }: { step?: number; onBack?: () => void; children: React.ReactNode }) {
  const { user } = useAuth();
  return (
    <>
      <div className="aq-wiz-top">
        <div className="wizard-topbar" aria-label="AI 앱 연결">
          {onBack
            ? <button type="button" className="wizard-topback" aria-label="이전으로 돌아가기" onClick={onBack}><BackIcon /></button>
            : <span className="wizard-topback aq-connect-topspace" aria-hidden="true" />}
          <span className="wizard-top-title">AI 앱 연결</span>
          <span className="aq-save-slot">
            {user?.email && <span className="aq-connect-account" title="이 계정으로 연결돼요">{user.email}</span>}
          </span>
        </div>
      </div>
      {children}
    </>
  );
}

function Header({ kicker, title, sub }: { kicker: string; title: React.ReactNode; sub?: React.ReactNode }) {
  return (
    <div className="wizard-header">
      <p className="aq-step-kicker">{kicker}</p>
      <h1 className="aq-step-title">{title}</h1>
      {sub && <p>{sub}</p>}
    </div>
  );
}

function Review({ request, d, onDone, onFailed }: {
  request: string; d: AiConnectDetails;
  onDone: (approved: boolean, granted: string[], redirect?: string) => void;
  onFailed: (err: unknown) => void;
}) {
  const confirm = useConfirm();
  const editable = d.workspaces.filter(w => w.role !== 'VIEWER');
  const [org, setOrg] = useState(() => (editable[0] ?? d.workspaces[0])?.id ?? '');
  const [on, setOn] = useState<Set<string>>(() => new Set(d.scopes.filter(s => s.checked).map(s => s.scope)));
  const [busy, setBusy] = useState<'approve' | 'deny' | null>(null);
  const offered = new Set(d.scopes.map(s => s.scope));
  const preset = PRESETS.find(p => {
    const want = new Set(['releases:read', ...p.scopes.filter(s => offered.has(s))]);
    return want.size === on.size && [...want].every(s => on.has(s));
  })?.id;

  const toggle = (scope: string, locked: boolean) => {
    if (locked) return;
    setOn(prev => {
      const next = new Set(prev);
      if (next.has(scope)) next.delete(scope); else next.add(scope);
      return next;
    });
  };
  const pick = (id: string) => {
    const p = PRESETS.find(x => x.id === id);
    if (p) setOn(new Set(['releases:read', ...p.scopes.filter(s => offered.has(s))]));
  };

  async function decide(approve: boolean) {
    if (busy) return;
    const risky = d.scopes.filter(s => s.risk !== 'none' && on.has(s.scope));
    if (approve && risky.length) {
      const ok = await confirm({
        title: `${risky.map(s => s.title).join('·')} 권한도 줄까요?`,
        message: risky.some(s => s.risk === 'money')
          ? '지급 신청은 대화에서 금액·수수료·받을 계좌를 내가 확인한 뒤에만 해요.'
          : '발매 신청은 대화에서 신청 내용을 내가 최종 확인한 뒤에만 접수해요.',
        confirmLabel: '허용하고 연결',
      });
      if (!ok) return;
    }
    if (!approve) {
      const ok = await confirm({ title: '연결하지 않을까요?', message: `${d.client.name}은 어데닉 계정에 접근할 수 없어요.`, confirmLabel: '연결하지 않기' });
      if (!ok) return;
    }
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

  const ws = d.workspaces.find(w => w.id === org);
  const verified = d.client.verified_host
    ? `${d.client.verified_host}에서 확인된 앱`
    : d.client.registered ? '어데닉에 등록된 에이전트' : null;
  const from = d.requested_from ? `${d.requested_from}에서 요청` : '연결 코드로 요청';

  return (
    <Frame step={-1}>
      <Header
        kicker="AI 앱 연결"
        title={`${d.client.name}에서\n어데닉을 쓰려고 해요`}
        sub="허용한 일만 할 수 있어요. 연결은 언제든 끊을 수 있어요."
      />

      <div className="aq-connect-app-row">
        <span className="aq-connect-app-mark" aria-hidden="true">{(d.client.name.match(/[\p{L}\p{N}]/u)?.[0] ?? 'A').toUpperCase()}</span>
        <span className="min-0">
          <strong>{d.client.name}</strong>
          <small>{verified ? `${verified} · ${from}` : from}</small>
        </span>
        {verified && <span className="aq-connect-tag is-ok">확인됨</span>}
      </div>
      {!verified && <p className="ui-callout is-warn small">어데닉이 이 앱이 누구인지 확인하지 못했어요. 내가 방금 시작한 연결일 때만 허용해 주세요.</p>}
      {d.device_code && (
        <div className="aq-connect-code-row">
          <span>AI 앱에 보이는 코드와 같은지 확인해 주세요</span>
          <strong>{d.device_code}</strong>
        </div>
      )}

      <h2 className="subhead">연결할 작업 공간</h2>
      {d.workspaces.length > 1 ? (
        <div className="aq-choice-row aq-connect-workspaces" role="radiogroup" aria-label="연결할 작업 공간">
          {d.workspaces.map(w => (
            <label key={w.id} className={`aq-choice${org === w.id ? ' is-on' : ''}`}>
              <input type="radio" name="aqConnectOrg" checked={org === w.id} onChange={() => setOrg(w.id)} />
              <strong>{workspaceName(w.name)}</strong>
              <small>{ROLE[w.role] ?? w.role}</small>
            </label>
          ))}
        </div>
      ) : (
        <div className="aq-choice is-on aq-connect-one-ws">
          <strong>{workspaceName(ws?.name ?? '')}</strong>
          <small>{ROLE[ws?.role ?? ''] ?? ws?.role}</small>
        </div>
      )}

      <div className="aq-connect-scope-head">
        <h2 className="subhead">허용할 일</h2>
        <div className="aq-yesno aq-connect-presets" role="radiogroup" aria-label="빠르게 고르기">
          {PRESETS.filter(p => p.id !== 'all' || d.scopes.length > 2).map(p => (
            <button key={p.id} type="button" role="radio" aria-checked={preset === p.id} className={`ui-chip${preset === p.id ? ' is-on' : ''}`} onClick={() => pick(p.id)}>
              {p.label}
            </button>
          ))}
        </div>
      </div>
      <div className="aq-options">
        {d.scopes.map(s => (
          <Fragment key={s.scope}>
            <label className={`aq-option${on.has(s.scope) ? ' is-on' : ''}${s.locked ? ' is-locked' : ''}`}>
              <span className="aq-option-text">
                <strong>
                  {s.title}
                  {s.locked && <span className="aq-connect-tag">항상 포함</span>}
                  {s.risk === 'submit' && <span className="aq-connect-tag is-warn">내 이름으로 접수</span>}
                  {s.risk === 'money' && <span className="aq-connect-tag is-warn">돈이 움직여요</span>}
                </strong>
                <small>{s.description}</small>
              </span>
              <input type="checkbox" aria-label={s.title} checked={on.has(s.scope)} disabled={s.locked} onChange={() => toggle(s.scope, s.locked)} />
            </label>
          </Fragment>
        ))}
      </div>
      {ws?.role === 'VIEWER' && <p className="ui-callout is-info small">{workspaceName(ws.name)}에서는 보기 전용이라, 허용해도 읽기만 할 수 있어요.</p>}

      <h2 className="subhead">어떤 권한을 줘도 지켜져요</h2>
      <ul className="aq-connect-promise">
        <li>지급 계좌를 바꾸거나 계약서·신청서에 서명할 수 없어요.</li>
        <li>발매 취소·서비스 중단·큰 금액 지급은 요청만 할 수 있어요. 매번 스튜디오에서 내가 승인해요.</li>
        <li>비밀번호·로그인 같은 계정 설정은 건드릴 수 없어요.</li>
        <li>프로필 → 연결된 AI 앱에서 언제든 끊을 수 있어요.</li>
      </ul>
      {(d.return_to || d.client.client_id.startsWith('https://')) && (
        <dl className="aq-connect-facts">
          {d.return_to && <><dt>승인 후 돌아갈 곳</dt><dd>{d.return_to}</dd></>}
          {d.client.client_id.startsWith('https://') && <><dt>앱 확인 주소</dt><dd>{d.client.client_id}</dd></>}
        </dl>
      )}

      <div className="step-actions">
        <button type="button" id="wizardBack" className="ui-btn is-soft" disabled={!!busy} onClick={() => void decide(false)}>거절</button>
        <button type="button" id="wizardNext" className={`ui-btn is-primary${busy === 'approve' ? ' is-busy' : ''}`} disabled={!!busy || !org} onClick={() => void decide(true)}>
          {`${on.size}개 허용하고 연결`}
        </button>
      </div>
      <p className="aq-connect-deny">
        <button type="button" className="ui-btn is-text" disabled={!!busy} onClick={() => void decide(false)}>연결하지 않을래요</button>
      </p>
    </Frame>
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
    <Frame step={-1}>
      <Header
        kicker="AI 앱 연결"
        title={approved ? '연결했어요' : '연결하지 않았어요'}
        sub={approved
          ? (redirect ? `${d.client.name} 대화로 돌아가고 있어요.` : `${d.client.name} 대화로 돌아가 이어 가 주세요.`)
          : `이 앱은 어데닉 계정에 접근할 수 없어요.${redirect ? ' AI 앱으로 돌아가고 있어요.' : ''}`}
      />
      {approved && (
        <>
          <h2 className="subhead">허용한 일</h2>
          <ul className="aq-connect-promise is-granted">{titles.map(t => <li key={t}>{t}</li>)}</ul>
          <p className="aq-option-note">바꾸거나 끊으려면 프로필 → 연결된 AI 앱으로 가세요.</p>
        </>
      )}
      <div className="step-actions">
        <Link id="wizardBack" className="ui-btn is-soft" to="/profile?open=ai">연결된 AI 앱</Link>
        {redirect
          ? <a id="wizardNext" className="ui-btn is-primary" href={redirect}>지금 돌아가기</a>
          : <Link id="wizardNext" className="ui-btn is-primary" to="/">스튜디오 홈</Link>}
      </div>
    </Frame>
  );
}

function Failed({ code, message }: { code: string; message: string }) {
  return (
    <Frame step={-1}>
      <Header kicker="AI 앱 연결" title={'연결 요청을\n열 수 없어요'} sub={message} />
      {code !== 'AI_CONNECT_CLIENT_UNKNOWN' && (
        <p className="ui-callout is-info small">AI 앱에서 연결을 처음부터 다시 시작해 주세요. 새 링크로 이 화면이 다시 열려요.</p>
      )}
      <div className="step-actions">
        <Link id="wizardNext" className="ui-btn is-primary" to="/">스튜디오 홈</Link>
      </div>
    </Frame>
  );
}

/** 기기 연결 — AI 앱이 알려 준 8자리 코드 */
function CodeEntry({ error, onSubmit }: { error?: string; onSubmit: (code: string) => void }) {
  const [value, setValue] = useState('');
  const clean = value.toUpperCase().replace(/[^A-Z]/g, '').slice(0, 8);
  const shown = clean.length > 4 ? `${clean.slice(0, 4)}-${clean.slice(4)}` : clean;
  return (
    <Frame step={-1}>
      <Header kicker="AI 앱 연결" title={'연결 코드를\n입력해 주세요'} sub="코드는 AI 앱이 대화에서 알려 줘요. 영문 8자리예요." />
      <form id="aqConnectCode" onSubmit={e => { e.preventDefault(); if (clean.length === 8) onSubmit(clean); }}>
        <div className="ui-field">
          <label htmlFor="aqConnectCodeInput">연결 코드 <span className="required">*</span></label>
          <input
            id="aqConnectCodeInput" className="aq-connect-code-input" value={shown} autoComplete="one-time-code"
            autoCapitalize="characters" spellCheck={false} placeholder="BCDF-GHJK"
            aria-invalid={!!error} onChange={e => setValue(e.target.value)}
          />
          {error && <p className="ui-field-help aq-help-error" role="alert">{error}</p>}
          <p className="ui-field-help">내가 시작하지 않은 연결이라면 입력하지 마세요.</p>
        </div>
      </form>
      <div className="step-actions">
        <Link id="wizardBack" className="ui-btn is-soft" to="/">취소</Link>
        <button type="submit" form="aqConnectCode" id="wizardNext" className="ui-btn is-primary" disabled={clean.length !== 8}>다음</button>
      </div>
    </Frame>
  );
}
