// 패스키 관리 — 등록한 패스키 목록, 추가(지금 비밀번호 확인), 삭제.
import { useEffect, useState } from 'react';
import { useToast } from './Toast';
import { useConfirm } from './Confirm';
import { addPasskey, defaultPasskeyName, listPasskeys, passkeySupported, removePasskey, type Passkey } from '../api/passkey';
import { errorMessage } from '../api/errors';
import { PasswordInput } from './AuthLayout';

const day = (iso: string) => new Date(iso).toLocaleDateString('ko-KR', { year: 'numeric', month: 'long', day: 'numeric' });

export function PasskeySettings() {
  const toast = useToast();
  const confirm = useConfirm();
  const [items, setItems] = useState<Passkey[] | null>(null);
  const [limit, setLimit] = useState(10);
  const [adding, setAdding] = useState(false);
  const [name, setName] = useState(defaultPasskeyName);
  const [password, setPassword] = useState('');
  const [showPw, setShowPw] = useState(false);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const supported = passkeySupported();

  useEffect(() => {
    listPasskeys()
      .then(r => { setItems(r.items); setLimit(r.limit); })
      .catch(err => setError(errorMessage(err, '패스키 목록을 불러오지 못했어요.')));
  }, []);

  async function add(e: React.FormEvent) {
    e.preventDefault();
    if (busy) return;
    setBusy(true);
    setError('');
    try {
      const r = await addPasskey(password, name);
      setItems(r.items);
      setAdding(false);
      setPassword('');
      toast('패스키를 등록했어요. 다음부터 이메일을 입력하고 패스키로 로그인할 수 있어요.');
    } catch (err) {
      setError(errorMessage(err, '패스키를 등록하지 못했어요.'));
    } finally {
      setBusy(false);
    }
  }

  async function remove(p: Passkey) {
    const ok = await confirm({ title: `‘${p.name}’ 패스키를 삭제할까요?`, message: '이 패스키로는 더 이상 로그인할 수 없어요. 기기에 남은 패스키는 기기 설정에서 지워 주세요.', confirmLabel: '삭제', danger: true });
    if (!ok) return;
    try {
      const r = await removePasskey(p.id);
      setItems(r.items);
      toast('패스키를 삭제했어요.', 'info');
    } catch (err) {
      toast(errorMessage(err, '패스키를 삭제하지 못했어요.'), 'error');
    }
  }

  return (
    <div className="aq-passkeys">
      <p className="aq-muted">
        패스키를 등록하면 로그인할 때 이메일을 입력하고 비밀번호 대신 이 기기의 지문·얼굴·화면 잠금으로 들어올 수 있어요.
        비밀번호보다 안전하고 피싱 사이트에서는 동작하지 않아요.
      </p>
      {error && <div className="ui-callout is-danger" role="alert">{error}</div>}
      {items === null && !error && <p role="status">불러오는 중이에요.</p>}
      {items && items.length > 0 && (
        <ul className="aq-passkey-list">
          {items.map(p => (
            <li key={p.id}>
              <span className="min-0">
                <strong>{p.name}</strong>
                <small>
                  {day(p.created_at)} 등록{p.last_used_at ? ` · ${day(p.last_used_at)} 사용` : ''}{p.synced ? ' · 다른 기기와 동기화됨' : ''}
                </small>
              </span>
              <button type="button" className="ui-btn is-soft is-sm" onClick={() => void remove(p)}>삭제</button>
            </li>
          ))}
        </ul>
      )}
      {items && items.length === 0 && <p>아직 등록한 패스키가 없어요.</p>}
      {!supported && <div className="ui-callout">이 브라우저에서는 패스키를 등록할 수 없어요. 최신 Chrome·Safari·Edge·Firefox에서 열어 주세요.</div>}
      {supported && items && items.length < limit && !adding && (
        <button type="button" className="ui-btn is-primary" onClick={() => { setAdding(true); setError(''); }}>패스키 추가</button>
      )}
      {adding && (
        <form onSubmit={add}>
          <div className="ui-field">
            <label htmlFor="passkeyName">이름</label>
            <input id="passkeyName" required maxLength={60} value={name} onChange={e => setName(e.target.value)} autoComplete="off" />
          </div>
          <div className="ui-field">
            <label htmlFor="passkeyPassword">지금 비밀번호</label>
            <PasswordInput
              id="passkeyPassword" required autoComplete="current-password" autoFocus
              value={password} onChange={e => setPassword(e.target.value)}
              show={showPw} onToggle={() => setShowPw(v => !v)}
            />
          </div>
          <div className="aq-row-actions">
            <button type="button" className="ui-btn is-soft" onClick={() => setAdding(false)} disabled={busy}>취소</button>
            <button type="submit" className={`ui-btn is-primary${busy ? ' is-busy' : ''}`} disabled={busy} aria-busy={busy}>
              {busy ? '기기에서 확인하는 중' : '기기에 패스키 만들기'}
            </button>
          </div>
        </form>
      )}
    </div>
  );
}
