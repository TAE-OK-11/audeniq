// 공동작업 수익 분배 요청 (/splits/:token) — 메일로 받은 링크. 내용은 로그인 없이 보이고,
// 수락은 내 AUDENIQ 계정(소유자)으로 한다. 거절은 로그인 없이 할 수 있다. 링크는 한 번 쓰면 끝난다.
// 같은 곡에 내 작사·작곡 지분이 적혀 있으면 그 확인도 여기서 함께 — 초대 하나로 수익 분배와 저작권 지분을 답한다
import { useEffect, useState } from 'react';
import { Link, useNavigate, useParams } from '../lib/router';
import { req } from '../api/http';
import { errorMessage } from '../api/errors';
import { useAuth } from '../api/auth';
import { MOCK } from '../lib/mode';
import { SkeletonLines } from '../components/Skeleton';
import { roleTextOf } from '../lib/publishing';
import type { ContributorRole } from '../api/publishing';
import '../styles/split/Publishing.css';

interface Invite {
  title: string; artist: string | null; track_title: string; from: string; name: string;
  share_bps: number; status: 'INVITED' | 'ACCEPTED' | 'DECLINED';
  /** 같은 곡에서 확인을 기다리는 내 작사·작곡 지분 */
  writer?: { title: string; share: number; roles: ContributorRole[] }[];
}
type WriterAnswer = 'yes' | 'no' | 'later';

export function SplitInvite() {
  const { token = '' } = useParams<{ token: string }>();
  const { user, orgs } = useAuth();
  const nav = useNavigate();
  const [inv, setInv] = useState<Invite | null>(null);
  const [gone, setGone] = useState(false);
  const [orgId, setOrgId] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [done, setDone] = useState<'' | 'ACCEPTED' | 'DECLINED'>('');
  const [writer, setWriter] = useState<WriterAnswer>('yes');
  const [note, setNote] = useState('');
  const owned = orgs.filter(o => !o.role || o.role === 'OWNER');
  const writerRows = inv?.writer ?? [];
  // 지분 답은 '맞아요'·'달라요'일 때만 보낸다 ('나중에'는 지분 확인 링크로 따로)
  const body = writerRows.length && writer !== 'later' ? { writer: writer === 'yes', note: writer === 'no' ? note.trim() || undefined : undefined } : {};
  const path = `/api/split-invites/${encodeURIComponent(token)}`;

  useEffect(() => {
    if (MOCK) { setInv({ title: '여름의 끝', artist: '한결', track_title: '여름의 끝', from: '한결 뮤직', name: '김작곡', share_bps: 3000, status: 'INVITED', writer: [{ title: '여름의 끝', share: 50, roles: ['composer', 'lyricist'] }] }); return; }
    req<Invite>(path, { quiet401: true }).then(setInv, () => setGone(true));
  }, [path]);
  useEffect(() => { if (!orgId && owned[0]) setOrgId(owned[0].id); }, [owned, orgId]);

  const accept = async () => {
    if (!user) { nav('/login', { state: { from: `/splits/${token}` } }); return; }
    if (!orgId) { setError('수락할 계정(소유자인 작업 공간)이 없어요.'); return; }
    setBusy(true); setError('');
    try {
      if (!MOCK) await req(`/api/orgs/${encodeURIComponent(orgId)}/split-invites/${encodeURIComponent(token)}/accept`, { method: 'POST', body });
      setDone('ACCEPTED');
    } catch (e) { setError(errorMessage(e)); } finally { setBusy(false); }
  };
  const decline = async () => {
    setBusy(true); setError('');
    try {
      if (!MOCK) await req(`${path}/decline`, { method: 'POST', body });
      setDone('DECLINED');
    } catch (e) { setError(errorMessage(e)); } finally { setBusy(false); }
  };

  const pct = inv ? `${(inv.share_bps / 100).toLocaleString(undefined, { maximumFractionDigits: 2 })}%` : '';
  return (
    <main className="aq-terms-page">
      <header className="aq-sign-top">
        <Link to="/" aria-label="AUDENIQ"><img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" /></Link>
        <span>수익 분배 요청</span>
      </header>
      <article className="aq-terms aq-split-invite">
        {gone ? (
          <><h1>이 링크는 쓸 수 없어요</h1><p className="muted">이미 처리됐거나, 새 요청 메일이 와서 바뀐 링크예요. 가장 최근 메일의 링크를 열어 주세요.</p></>
        ) : !inv ? <SkeletonLines /> : done ? (
          <div className="ui-callout is-success" role="status">
            {done === 'ACCEPTED'
              ? <>수락했어요. ‘{inv.track_title}’ 수익 중 {pct}가 정산될 때마다 내 잔액에 쌓여요. <Link to="/settlement">정산·지급 보기</Link></>
              : <>거절했어요. 그 몫은 신청한 분에게 돌아가요.</>}
            {writerRows.length > 0 && writer !== 'later' && <> {writer === 'yes' ? '작사·작곡 지분도 맞다고 알렸어요.' : '작사·작곡 지분이 다르다고 알렸어요. 신청한 분이 확인하고 고칠 거예요.'}</>}
          </div>
        ) : (
          <>
            <h1>{inv.from}에서 ‘{inv.track_title}’ 수익의 {pct}를 나눠 드리려고 해요</h1>
            <dl className="ui-kv">
              <div><dt>발매</dt><dd>{inv.title}{inv.artist ? ` · ${inv.artist}` : ''}</dd></div>
              <div><dt>곡</dt><dd>{inv.track_title}</dd></div>
              <div><dt>받는 분</dt><dd>{inv.name}</dd></div>
              <div><dt>내 몫</dt><dd><b>{pct}</b> (AUDENIQ 수수료를 뺀 정산 수익 기준)</dd></div>
            </dl>
            <p className="small muted">
              수락하면 이 곡 수익이 정산될 때마다 내 계정 잔액에 쌓이고, 내 계좌로 출금할 수 있어요. 세금도 내 계정 정보로 처리돼요.
              수락 전에 정산된 몫은 보관했다가 수락할 때 함께 옮겨 드려요.
            </p>
            {inv.status !== 'INVITED' ? <p className="muted">이미 처리된 요청이에요.</p> : (
              <>
                {writerRows.length > 0 && (
                  <fieldset className="aq-split-writer">
                    <legend>작사·작곡 지분도 맞나요?</legend>
                    <p className="small muted">{inv.from}에서 이 곡의 저작권 관리를 위해 적은 내 창작 지분이에요. 수락·거절과 상관없이 함께 답할 수 있어요.</p>
                    <ul className="aq-split-writer-list">
                      {writerRows.map(w => <li key={w.title}><strong>‘{w.title}’</strong> {roleTextOf(w.roles)} · <b>{w.share.toLocaleString(undefined, { maximumFractionDigits: 2 })}%</b></li>)}
                    </ul>
                    {([['yes', '맞아요'], ['no', '달라요'], ['later', '나중에 답할게요']] as const).map(([v, label]) => (
                      <label key={v} className="ui-check is-plain">
                        <input type="radio" name="siWriter" checked={writer === v} onChange={() => setWriter(v)} />
                        <span>{label}</span>
                      </label>
                    ))}
                    {writer === 'no' && (
                      <textarea className="ui-input" aria-label="다른 점" rows={3} maxLength={1000} placeholder="예) 작곡에도 참여했어요. 제 지분은 40%예요." value={note} onChange={e => setNote(e.target.value)} />
                    )}
                  </fieldset>
                )}
                {user && owned.length > 1 && (
                  <div className="ui-field">
                    <label htmlFor="siOrg">받을 계정</label>
                    <select id="siOrg" className="ui-select" value={orgId} onChange={e => setOrgId(e.target.value)}>
                      {owned.map(o => <option key={o.id} value={o.id}>{o.name}</option>)}
                    </select>
                  </div>
                )}
                {error && <p className="aq-smartlink-alert-err" role="alert">{error}</p>}
                <div className="aq-confirm-actions">
                  <button type="button" className="ui-btn is-soft" disabled={busy} onClick={() => void decline()}>거절</button>
                  <button type="button" className="ui-btn is-primary" disabled={busy} onClick={() => void accept()}>
                    {user ? '수락하기' : '로그인하고 수락하기'}
                  </button>
                </div>
                {!user && <p className="small muted">계정이 없으면 <Link to="/signup" state={{ from: `/splits/${token}` }}>가입</Link>한 뒤 이 링크로 돌아와 수락해 주세요.</p>}
              </>
            )}
          </>
        )}
      </article>
    </main>
  );
}
