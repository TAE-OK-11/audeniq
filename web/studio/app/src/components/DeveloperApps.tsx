// 개발자 앱 — 내 서비스에서 AUDENIQ API(/api/v1)를 쓰는 OAuth 2.0 앱을 만들고 관리한다.
// MCP와 같은 일을 같은 권한으로 열어 준다. 앱은 처음에 만든 사람만 연결할 수 있고,
// 검토를 요청해 운영자가 승인하면 다른 아티스트도 연결할 수 있다.
// 비밀 키(client secret)는 만들 때와 새로 만들 때 한 번만 보여 준다.
import { useEffect, useRef, useState } from 'react';
import { useToast } from './Toast';
import { useConfirm } from './Confirm';
import { CopyButton } from './CopyButton';
import { ApiError, errorMessage } from '../api/errors';
import {
  createDeveloperApp, deleteDeveloperApp, developerApps, requestDeveloperAppReview, rotateDeveloperAppSecret,
  updateDeveloperApp, VISIBILITY_LABEL,
  type DeveloperApp, type DeveloperApps as AppsInfo,
} from '../api/developerApps';
import { Badge } from '../ui';

const lines = (v: string) => v.split('\n').map(l => l.trim()).filter(Boolean);
const TONE = { PRIVATE: 'gray', REVIEW: 'amber', PUBLIC: 'green' } as const;

function AppForm({ onCreated, onClose }: { onCreated: (a: DeveloperApp) => void; onClose: () => void }) {
  const [name, setName] = useState('');
  const [redirects, setRedirects] = useState('');
  const [website, setWebsite] = useState('');
  const [description, setDescription] = useState('');
  const [publicClient, setPublicClient] = useState(false);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (busy) return;
    setBusy(true);
    setError('');
    try {
      onCreated(await createDeveloperApp({
        name: name.trim(), redirect_uris: lines(redirects),
        website: website.trim() || undefined, description: description.trim() || undefined,
        public_client: publicClient,
      }));
    } catch (err) {
      setError(errorMessage(err, '앱을 만들지 못했어요.'));
    } finally {
      setBusy(false);
    }
  }

  return (
    <form className="aq-token-form" onSubmit={submit}>
      <div className="ui-field">
        <label htmlFor="aqAppName">앱 이름</label>
        <input id="aqAppName" required maxLength={60} value={name} onChange={e => setName(e.target.value)} placeholder="예: 우리 레이블 발매 도우미" autoComplete="off" autoFocus />
        <p className="ui-field-help">연결 화면에서 아티스트가 보는 이름이에요. 만든 뒤에는 바꿀 수 없어요.</p>
      </div>
      <div className="aq-token-group">
        <p id="aqAppKind" className="ui-field-label">앱 종류</p>
        <div className="ui-chips is-wrap" role="radiogroup" aria-labelledby="aqAppKind">
          {[{ v: false, label: '서버 앱' }, { v: true, label: '모바일·데스크톱 앱' }].map(o => (
            <button key={o.label} type="button" role="radio" aria-checked={publicClient === o.v}
              className={`ui-chip${publicClient === o.v ? ' is-on' : ''}`} onClick={() => setPublicClient(o.v)}>
              {o.label}
            </button>
          ))}
        </div>
        <p className="ui-field-help">
          {publicClient
            ? '비밀 키 없이 PKCE로만 연결해요. 앱 안에 비밀 키를 넣을 수 없을 때 골라 주세요.'
            : '서버에 비밀 키를 두고 연결해요. 웹 서비스라면 이쪽이에요.'}
        </p>
      </div>
      <div className="ui-field">
        <label htmlFor="aqAppRedirects">돌아갈 주소</label>
        <textarea id="aqAppRedirects" required rows={2} value={redirects} onChange={e => setRedirects(e.target.value)} placeholder="https://myapp.example/oauth/callback" spellCheck={false} />
        <p className="ui-field-help">연결을 마치면 이 주소로 돌아가요. 한 줄에 하나씩, 10개까지 적을 수 있어요.</p>
      </div>
      <div className="ui-field">
        <label htmlFor="aqAppWebsite">웹사이트 <span className="aq-optional">선택</span></label>
        <input id="aqAppWebsite" type="url" value={website} onChange={e => setWebsite(e.target.value)} placeholder="https://myapp.example" autoComplete="off" />
      </div>
      <div className="ui-field">
        <label htmlFor="aqAppDescription">앱 설명 <span className="aq-optional">선택</span></label>
        <textarea id="aqAppDescription" rows={2} maxLength={500} value={description} onChange={e => setDescription(e.target.value)} placeholder="이 앱이 AUDENIQ에서 하는 일을 적어 주세요." />
      </div>
      {error && <p className="ui-field-help aq-help-error" role="alert">{error}</p>}
      <div className="aq-row-actions">
        <button type="button" className="ui-btn is-soft" onClick={onClose} disabled={busy}>닫기</button>
        <button type="submit" className={`ui-btn is-primary${busy ? ' is-busy' : ''}`} disabled={busy || !name.trim() || !lines(redirects).length} aria-busy={busy}>
          {busy ? '만드는 중' : '앱 만들기'}
        </button>
      </div>
    </form>
  );
}

/** 만든 직후·비밀 키를 새로 만든 직후 — 한 번만 보여 준다 */
function Credentials({ app, onClose }: { app: DeveloperApp; onClose: () => void }) {
  const title = useRef<HTMLParagraphElement>(null);
  useEffect(() => { title.current?.focus(); }, []);
  return (
    <div className="ui-callout is-success aq-token-made">
      <p role="status" tabIndex={-1} ref={title}><strong>{app.client_secret ? `‘${app.name}’의 비밀 키를 만들었어요.` : `‘${app.name}’ 앱을 만들었어요.`}</strong></p>
      {app.client_secret && <p>이 창을 닫으면 비밀 키를 다시 볼 수 없어요. 지금 서버 설정에 넣어 주세요.</p>}
      <div className="aq-token-value">
        <span className="ui-field-label">클라이언트 ID</span>
        <code>{app.client_id}</code>
        <CopyButton text={app.client_id} />
      </div>
      {app.client_secret && (
        <div className="aq-token-value">
          <span className="ui-field-label">비밀 키 (client secret)</span>
          <code>{app.client_secret}</code>
          <CopyButton text={app.client_secret} label="키 복사" />
        </div>
      )}
      <p className="aq-muted">비밀 키는 비밀번호처럼 다뤄 주세요. 앱 코드나 저장소에 그대로 넣지 말고 서버 설정에만 두세요.</p>
      <button type="button" className="ui-btn is-soft is-sm" onClick={onClose}>닫기</button>
    </div>
  );
}

function AppDetail({ app, onChanged, onDeleted, onSecret }: {
  app: DeveloperApp; onChanged: (a: DeveloperApp) => void; onDeleted: () => void; onSecret: (a: DeveloperApp) => void;
}) {
  const toast = useToast();
  const confirm = useConfirm();
  const [redirects, setRedirects] = useState(app.redirect_uris.join('\n'));
  const [busy, setBusy] = useState<string | null>(null);
  const changed = redirects.trim() !== app.redirect_uris.join('\n');

  async function run(key: string, work: () => Promise<void>) {
    if (busy) return;
    setBusy(key);
    try { await work(); } catch (err) { toast(errorMessage(err, '처리하지 못했어요. 잠시 후 다시 해 주세요.'), 'error'); } finally { setBusy(null); }
  }

  return (
    <div className="aq-dev-detail">
      <div className="aq-token-value">
        <span className="ui-field-label">클라이언트 ID</span>
        <code>{app.client_id}</code>
        <CopyButton text={app.client_id} />
      </div>
      {app.review_note && (
        <div className="ui-callout is-warn" role="note"><p><strong>검토 결과</strong> {app.review_note}</p></div>
      )}
      <div className="ui-field">
        <label htmlFor={`aqRedirects-${app.id}`}>돌아갈 주소</label>
        <textarea id={`aqRedirects-${app.id}`} rows={2} value={redirects} onChange={e => setRedirects(e.target.value)} spellCheck={false} />
      </div>
      <div className="aq-dev-actions">
        {changed && (
          <button type="button" className={`ui-btn is-primary is-sm${busy === 'save' ? ' is-busy' : ''}`} disabled={!!busy}
            onClick={() => void run('save', async () => { onChanged(await updateDeveloperApp(app.id, { redirect_uris: lines(redirects) })); toast('돌아갈 주소를 저장했어요.', 'success'); })}>
            주소 저장하기
          </button>
        )}
        {!app.public_client && (
          <button type="button" className="ui-btn is-soft is-sm" disabled={!!busy}
            onClick={() => void run('secret', async () => {
              const ok = await confirm({ title: '비밀 키를 새로 만들까요?', message: '지금 쓰는 비밀 키는 바로 멈춰요. 새 키를 서버에 넣을 때까지 이 앱은 연결을 이어 가지 못해요.', confirmLabel: '새로 만들기' });
              if (ok) onSecret(await rotateDeveloperAppSecret(app.id));
            })}>
            비밀 키 새로 만들기
          </button>
        )}
        {app.visibility === 'PRIVATE' && (
          <button type="button" className="ui-btn is-soft is-sm" disabled={!!busy}
            onClick={() => void run('review', async () => {
              const ok = await confirm({ title: '공개 검토를 요청할까요?', message: '승인되면 다른 아티스트도 이 앱을 연결할 수 있어요. 검토하는 동안에도 나는 그대로 쓸 수 있어요.', confirmLabel: '검토 요청하기' });
              if (ok) { onChanged(await requestDeveloperAppReview(app.id)); toast('공개 검토를 요청했어요.', 'success'); }
            })}>
            공개 검토 요청하기
          </button>
        )}
        <button type="button" className="ui-btn is-text is-sm aq-dev-delete" disabled={!!busy}
          onClick={() => void run('delete', async () => {
            const ok = await confirm({
              title: `‘${app.name}’ 앱을 지울까요?`,
              message: app.connections > 0
                ? `이 앱을 연결한 ${app.connections}명의 연결이 모두 끊겨요. 지운 앱은 되살릴 수 없어요.`
                : '지운 앱은 되살릴 수 없어요.',
              confirmLabel: '지우기', danger: true,
            });
            if (ok) { await deleteDeveloperApp(app.id); onDeleted(); toast('앱을 지웠어요.', 'info'); }
          })}>
          앱 지우기
        </button>
      </div>
    </div>
  );
}

export function DeveloperApps() {
  const [info, setInfo] = useState<AppsInfo | null>(null);
  const [error, setError] = useState('');
  const [creating, setCreating] = useState(false);
  const [open, setOpen] = useState<string | null>(null);
  const [made, setMade] = useState<DeveloperApp | null>(null);

  useEffect(() => {
    developerApps().then(setInfo).catch(err => setError(err instanceof ApiError && err.status === 404
      ? '지금은 개발자 앱을 쓸 수 없어요.'
      : errorMessage(err, '개발자 앱을 불러오지 못했어요.')));
  }, []);

  const replace = (a: DeveloperApp) => setInfo(v => v && { ...v, items: v.items.map(i => (i.id === a.id ? { ...a, client_secret: undefined } : i)) });

  if (error) return <div className="ui-callout is-danger" role="alert">{error}</div>;
  if (!info) return <p className="aq-muted" role="status">불러오고 있어요…</p>;
  const full = info.items.length >= info.limit;

  return (
    <div className="aq-passkeys aq-dev-apps">
      <p className="aq-muted">
        내 서비스에서 AUDENIQ API를 쓰려면 앱을 만들어 OAuth 2.0으로 연결해요. AI 앱(MCP)과 같은 일을 같은 권한으로 할 수 있어요.
        처음에는 나만 연결할 수 있고, 다른 아티스트에게 열려면 공개 검토를 받아요.
      </p>

      {made && <Credentials app={made} onClose={() => setMade(null)} />}

      {creating ? (
        <AppForm
          onClose={() => setCreating(false)}
          onCreated={a => {
            setInfo(v => v && { ...v, items: [{ ...a, client_secret: undefined }, ...v.items] });
            setCreating(false);
            setMade(a);
          }}
        />
      ) : info.items.length === 0 ? (
        <div className="aq-token-empty">
          <strong>아직 만든 앱이 없어요</strong>
          <small>앱을 만들면 클라이언트 ID와 비밀 키를 받아요.</small>
        </div>
      ) : (
        <ul className="aq-passkey-list">
          {info.items.map(a => (
            <li key={a.id} className={open === a.id ? 'is-open' : undefined}>
              <span className="min-0">
                <strong>{a.name}</strong>
                <small>{[a.public_client ? '모바일·데스크톱' : '서버 앱', a.connections ? `${a.connections}명 연결` : '연결 없음'].join(' · ')}</small>
              </span>
              <Badge tone={TONE[a.visibility]} dot>{VISIBILITY_LABEL[a.visibility]}</Badge>
              <button type="button" className="ui-btn is-soft is-sm" aria-expanded={open === a.id} onClick={() => setOpen(o => (o === a.id ? null : a.id))}>
                {open === a.id ? '접기' : '관리'}
              </button>
              {open === a.id && (
                <AppDetail
                  app={a}
                  onChanged={replace}
                  onSecret={x => { replace(x); setMade(x); }}
                  onDeleted={() => { setInfo(v => v && { ...v, items: v.items.filter(i => i.id !== a.id) }); setOpen(null); }}
                />
              )}
            </li>
          ))}
        </ul>
      )}

      {!creating && (
        <button type="button" className="ui-btn is-primary aq-passkey-btn" disabled={full} onClick={() => { setMade(null); setCreating(true); }}>
          {full ? `앱은 ${info.limit}개까지 만들 수 있어요` : '앱 만들기'}
        </button>
      )}

      <details className="aq-token-exchange aq-dev-endpoints">
        <summary>연결 정보</summary>
        {[
          ['API 주소', info.api_base],
          ['인증 주소', info.authorization_endpoint],
          ['토큰 주소', info.token_endpoint],
          ['resource', info.resource],
        ].map(([label, value]) => (
          <div key={label} className="aq-token-value">
            <span className="ui-field-label">{label}</span>
            <code>{value}</code>
            <CopyButton text={value} />
          </div>
        ))}
        <p className="aq-muted">
          authorization code + PKCE(S256)로 연결하고, 토큰을 받을 때 resource에 API 주소를 넣어요.
          {' '}<a href={info.openapi} target="_blank" rel="noreferrer">API 문서(OpenAPI) 보기</a>
        </p>
      </details>
    </div>
  );
}
