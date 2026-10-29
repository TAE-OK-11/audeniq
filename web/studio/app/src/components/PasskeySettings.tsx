// 패스키 관리 — 등록한 패스키 목록, 추가(지금 비밀번호 확인), 삭제.
import { useEffect, useState } from 'react';
import { useToast } from './Toast';
import { useConfirm } from './Confirm';
import { abortPasskeyCeremony, addPasskey, defaultPasskeyName, listPasskeys, passkeySupported, removePasskey, type Passkey } from '../api/passkey';
import { errorMessage } from '../api/errors';
import { PasswordInput } from './AuthLayout';
import { Reveal } from '../ui';
import { Glyph } from './Glyph';
import { relativeTime } from '../lib/date';
import { SkeletonAcctRows } from './Skeleton';

const day = (iso: string) => new Date(iso).toLocaleDateString('ko-KR', { month: 'long', day: 'numeric' });

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
  useEffect(() => () => abortPasskeyCeremony(), []);
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
      toast('패스키를 등록했어요. 다음부터 비밀번호 없이 로그인할 수 있어요.');
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

  // 계정 정보의 다른 설정 창(연결된 계정·AI 앱)과 같은 줄 — 아이콘 칸 · 이름 · 한 줄 정보 · 오른쪽 조용한 글자 버튼(지우는 일은 빨강)
  return (
    <div className="aq-passkeys">
      <p className="aq-muted">
        지문·얼굴·화면 잠금(PIN)으로 비밀번호 없이 로그인해요. 피싱 사이트에서는 동작하지 않아 비밀번호보다 안전해요.
      </p>
      {error && <div className="ui-callout is-danger" role="alert">{error}</div>}
      {items === null && !error && <SkeletonAcctRows count={1} />}
      {items && items.length > 0 && (
        <ul className="aq-acct-list">
          {items.map(p => (
            <li key={p.id}>
              <span className="aq-acct-ico" aria-hidden="true"><Glyph name="key" size={18} /></span>
              <span className="min-0 aq-acct-copy">
                <strong>{p.name}{p.synced && <em className="aq-acct-tag">동기화</em>}</strong>
                <small>{p.last_used_at ? `마지막 사용 ${relativeTime(p.last_used_at)}` : '아직 사용 안 함'} · {day(p.created_at)} 등록</small>
              </span>
              <button type="button" className="ui-btn is-text is-sm aq-danger-text" onClick={() => void remove(p)}>삭제</button>
            </li>
          ))}
        </ul>
      )}
      {items && items.length === 0 && (
        <div className="aq-acct-empty">
          <span className="aq-acct-ico" aria-hidden="true"><Glyph name="key" size={18} /></span>
          <span><strong>아직 등록한 패스키가 없어요</strong><small>이 기기에 만들어 두면 다음부터 바로 로그인해요.</small></span>
        </div>
      )}
      {!supported && <div className="ui-callout">이 브라우저에서는 패스키를 등록할 수 없어요. 최신 Chrome·Safari·Edge·Firefox에서 열어 주세요.</div>}
      {supported && items && items.length < limit && (
        <Reveal open={!adding} focus="first">
          <button type="button" className="ui-btn is-soft is-block" aria-expanded={adding} onClick={() => { setAdding(true); setError(''); }}>
            <Glyph name="plus" size={16} />이 기기에 패스키 추가
          </button>
        </Reveal>
      )}
      <Reveal open={adding}>
        <form onSubmit={add} className="aq-acct-form">
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
            <button type="button" className="ui-btn is-soft" onClick={() => setAdding(false)} disabled={busy}>닫기</button>
            <button type="submit" className={`ui-btn is-primary${busy ? ' is-busy' : ''}`} disabled={busy} aria-busy={busy}>
              {busy ? '기기에서 확인하는 중' : '기기에 패스키 만들기'}
            </button>
          </div>
        </form>
      </Reveal>
    </div>
  );
}
