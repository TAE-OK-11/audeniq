// 공지·이벤트 관리 — D1(CONTENT_DB)에 글을 쓰고 고치고 내린다.
// 로그인 대신 Worker 시크릿 CONTENT_ADMIN_TOKEN으로 인증한다 (토큰은 이 탭의 sessionStorage에만 보관).
import { useCallback, useEffect, useState, type FormEvent } from 'react';
import { Link } from '../lib/router';
import { useToast } from '../components/Toast';
import { useConfirm } from '../components/Confirm';
import { errorMessage } from '../api/errors';
import {
  contentAdmin, type AdminEvent, type AdminNotice, type ContentKind, type EventInput, type NoticeInput,
} from '../api/content';

const TOKEN_KEY = 'aq.content-admin-token';
const readToken = () => { try { return sessionStorage.getItem(TOKEN_KEY) ?? ''; } catch { return ''; } };
const writeToken = (t: string) => { try { if (t) sessionStorage.setItem(TOKEN_KEY, t); else sessionStorage.removeItem(TOKEN_KEY); } catch { /* 저장 불가 환경 */ } };

const KST = 9 * 3_600_000;
/** UTC ISO → datetime-local 값 (KST) */
export function toKstInput(iso: string): string {
  const t = Date.parse(iso);
  return Number.isNaN(t) ? '' : new Date(t + KST).toISOString().slice(0, 16);
}
/** datetime-local 값 (KST) → UTC ISO (초 단위, 끝에 Z) */
export function fromKstInput(v: string): string {
  const t = Date.parse(`${v}:00Z`);
  return Number.isNaN(t) ? '' : new Date(t - KST).toISOString().replace(/\.\d{3}Z$/, 'Z');
}
const nowKstInput = () => new Date(Date.now() + KST).toISOString().slice(0, 16);
const kstLabel = (iso: string) => toKstInput(iso).replace('T', ' ');

type Row = AdminNotice | AdminEvent;
type State = 'live' | 'scheduled' | 'deleted';
const stateOf = (r: Row, now: string): State => (r.deleted_at ? 'deleted' : r.published_at > now ? 'scheduled' : 'live');
const STATE_LABEL: Record<State, string> = { live: '게시 중', scheduled: '예약', deleted: '내림' };
const EVENT_STATUS: Record<string, string> = { upcoming: '예정', ongoing: '진행 중', ended: '종료' };

interface Draft {
  id: string | null;
  title: string;
  body: string;
  pinned: boolean;
  publishedAt: string; // datetime-local (KST)
  summary: string;
  place: string;
  startsOn: string;
  endsOn: string;
  linkUrl: string;
  deleted: boolean;
}

const emptyDraft = (): Draft => ({
  id: null, title: '', body: '', pinned: false, publishedAt: nowKstInput(),
  summary: '', place: '', startsOn: nowKstInput().slice(0, 10), endsOn: '', linkUrl: '', deleted: false,
});

function draftOf(kind: ContentKind, r: Row): Draft {
  const base = { ...emptyDraft(), id: r.id, title: r.title, body: r.body, publishedAt: toKstInput(r.published_at), deleted: !!r.deleted_at };
  if (kind === 'notices') return { ...base, pinned: !!(r as AdminNotice).pinned };
  const e = r as AdminEvent;
  return { ...base, summary: e.summary, place: e.place, startsOn: e.starts_on, endsOn: e.ends_on ?? '', linkUrl: e.link_url ?? '' };
}

function inputOf(kind: ContentKind, d: Draft): NoticeInput | EventInput {
  const published_at = fromKstInput(d.publishedAt);
  if (kind === 'notices') return { title: d.title, body: d.body, pinned: d.pinned, published_at };
  return {
    title: d.title, summary: d.summary, body: d.body, place: d.place,
    starts_on: d.startsOn, ends_on: d.endsOn || null, link_url: d.linkUrl.trim() || null, published_at,
  };
}

function TokenGate({ onReady }: { onReady: (token: string) => void }) {
  const [value, setValue] = useState('');
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState('');
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    const t = value.trim();
    if (!t) { setErr('관리자 토큰을 입력해 주세요.'); return; }
    setBusy(true); setErr('');
    try {
      await contentAdmin.list(t, 'notices');
      writeToken(t);
      onReady(t);
    } catch (e2) {
      setErr(errorMessage(e2, '확인하지 못했어요.'));
    } finally {
      setBusy(false);
    }
  };
  return (
    <form className="aq-cadmin-gate" onSubmit={submit}>
      <h1>콘텐츠 관리</h1>
      <p>공지사항·이벤트를 올리려면 Worker에 설정한 관리자 토큰(CONTENT_ADMIN_TOKEN)을 입력해 주세요.</p>
      <div className="field">
        <label htmlFor="cadminToken">관리자 토큰</label>
        <input
          id="cadminToken" type="password" autoComplete="off" spellCheck={false}
          value={value} onChange={e => setValue(e.target.value)} aria-invalid={!!err}
        />
        {err && <p className="help aq-help-error" role="alert">{err}</p>}
      </div>
      <button type="submit" className="button" disabled={busy}>{busy ? '확인하는 중' : '확인'}</button>
    </form>
  );
}

function Editor({ kind, draft, saving, onChange, onSave, onCancel, onDelete }: {
  kind: ContentKind; draft: Draft; saving: boolean;
  onChange: (d: Draft) => void; onSave: () => void; onCancel: () => void; onDelete: () => void;
}) {
  const set = <K extends keyof Draft>(k: K, v: Draft[K]) => onChange({ ...draft, [k]: v });
  const notice = kind === 'notices';
  const scheduled = fromKstInput(draft.publishedAt) > new Date().toISOString();
  return (
    <form className="aq-cadmin-editor" onSubmit={e => { e.preventDefault(); onSave(); }}>
      <div className="aq-cadmin-editor-head">
        <h2>{draft.id ? (notice ? '공지 수정' : '이벤트 수정') : (notice ? '새 공지' : '새 이벤트')}</h2>
        {draft.deleted && <span className="status-chip">내린 글 · 저장하면 다시 게시돼요</span>}
      </div>
      <div className="field">
        <label htmlFor="caTitle">제목 <span className="required">*</span></label>
        <input id="caTitle" required maxLength={200} value={draft.title} onChange={e => set('title', e.target.value)} />
      </div>
      {!notice && (
        <>
          <div className="field">
            <label htmlFor="caSummary">한 줄 소개</label>
            <input id="caSummary" maxLength={300} value={draft.summary} onChange={e => set('summary', e.target.value)} />
          </div>
          <div className="aq-cadmin-grid">
            <div className="field">
              <label htmlFor="caStarts">시작일 <span className="required">*</span></label>
              <input id="caStarts" type="date" required value={draft.startsOn} onChange={e => set('startsOn', e.target.value)} />
            </div>
            <div className="field">
              <label htmlFor="caEnds">종료일</label>
              <input id="caEnds" type="date" min={draft.startsOn} value={draft.endsOn} onChange={e => set('endsOn', e.target.value)} />
            </div>
            <div className="field">
              <label htmlFor="caPlace">장소</label>
              <input id="caPlace" maxLength={120} placeholder="AUDENIQ STUDIO" value={draft.place} onChange={e => set('place', e.target.value)} />
            </div>
          </div>
          <div className="field">
            <label htmlFor="caLink">자세히 보기 링크</label>
            <input id="caLink" type="url" maxLength={500} placeholder="https://" value={draft.linkUrl} onChange={e => set('linkUrl', e.target.value)} />
          </div>
        </>
      )}
      <div className="field">
        <label htmlFor="caBody">본문</label>
        <textarea id="caBody" rows={12} maxLength={20000} value={draft.body} onChange={e => set('body', e.target.value)} />
        <p className="help">줄바꿈은 그대로 보여요. {draft.body.length.toLocaleString()} / 20,000자</p>
      </div>
      <div className="aq-cadmin-grid">
        <div className="field">
          <label htmlFor="caPublished">게시 시각 (한국 시간)</label>
          <input id="caPublished" type="datetime-local" required value={draft.publishedAt} onChange={e => set('publishedAt', e.target.value)} />
          <p className="help">{scheduled ? '이 시각이 되면 자동으로 보여요.' : '저장하면 바로 보여요.'}</p>
        </div>
        {notice && (
          <label className="aq-cadmin-check">
            <input type="checkbox" checked={draft.pinned} onChange={e => set('pinned', e.target.checked)} />
            <span>중요 공지로 상단에 고정</span>
          </label>
        )}
      </div>
      <div className="aq-cadmin-actions">
        {draft.id && !draft.deleted && <button type="button" className="button ghost aq-cadmin-danger" onClick={onDelete} disabled={saving}>내리기</button>}
        <span className="aq-cadmin-spacer" />
        <button type="button" className="button secondary" onClick={onCancel} disabled={saving}>취소</button>
        <button type="submit" className="button" disabled={saving}>
          {saving ? '저장하는 중' : draft.deleted ? '다시 게시' : scheduled ? '예약 저장' : draft.id ? '저장' : '게시하기'}
        </button>
      </div>
    </form>
  );
}

export function ContentAdmin() {
  const toast = useToast();
  const confirm = useConfirm();
  const [token, setToken] = useState(readToken);
  const [kind, setKind] = useState<ContentKind>('notices');
  const [rows, setRows] = useState<Row[] | null>(null);
  const [now, setNow] = useState('');
  const [loadError, setLoadError] = useState('');
  const [draft, setDraft] = useState<Draft | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => { document.title = '콘텐츠 관리 | AUDENIQ STUDIO'; }, []);

  const lock = useCallback(() => { writeToken(''); setToken(''); setRows(null); setDraft(null); }, []);

  const load = useCallback(async () => {
    if (!token) return;
    setLoadError('');
    try {
      const r = await contentAdmin.list(token, kind);
      setRows(r.items);
      setNow(r.now);
    } catch (e) {
      if ((e as { status?: number }).status === 401) { lock(); toast('관리자 토큰을 다시 입력해 주세요.', 'error'); return; }
      setLoadError(errorMessage(e, '목록을 불러오지 못했어요.'));
    }
  }, [token, kind, lock, toast]);

  useEffect(() => { setRows(null); void load(); }, [load]);

  if (!token) return <div className="aq-cadmin"><TokenGate onReady={setToken} /></div>;

  const save = async () => {
    if (!draft) return;
    if (!draft.title.trim()) { toast('제목을 입력해 주세요.', 'error'); return; }
    const input = inputOf(kind, draft);
    if (!input.published_at) { toast('게시 시각을 확인해 주세요.', 'error'); return; }
    setSaving(true);
    try {
      if (draft.id) await contentAdmin.update(token, kind, draft.id, input);
      else await contentAdmin.create(token, kind, input);
      toast(draft.id ? '저장했어요.' : '게시했어요.', 'success');
      setDraft(null);
      await load();
    } catch (e) {
      toast(errorMessage(e, '저장하지 못했어요.'), 'error');
    } finally {
      setSaving(false);
    }
  };

  const remove = async () => {
    if (!draft?.id) return;
    const ok = await confirm({
      title: '이 글을 내릴까요?',
      message: '공지·이벤트 화면에서 바로 사라져요. 목록에 남아 있어 나중에 다시 게시할 수 있어요.',
      confirmLabel: '내리기',
      danger: true,
    });
    if (!ok) return;
    setSaving(true);
    try {
      await contentAdmin.remove(token, kind, draft.id);
      toast('글을 내렸어요.', 'success');
      setDraft(null);
      await load();
    } catch (e) {
      toast(errorMessage(e, '내리지 못했어요.'), 'error');
    } finally {
      setSaving(false);
    }
  };

  const switchKind = (k: ContentKind) => { if (k !== kind) { setKind(k); setDraft(null); } };
  const viewHref = (r: Row) => (kind === 'notices' ? `/notices/${encodeURIComponent(r.id)}` : '/events');

  return (
    <div className="aq-cadmin">
      <header className="aq-cadmin-top">
        <Link to="/" className="aq-cadmin-brand" aria-label="AUDENIQ STUDIO">
          <img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" />
        </Link>
        <span className="aq-cadmin-title">콘텐츠 관리</span>
        <button type="button" className="button ghost" onClick={lock}>잠그기</button>
      </header>

      <div className="tabs aq-tabs" role="tablist" aria-label="콘텐츠 종류">
        <button type="button" role="tab" className="tab" aria-selected={kind === 'notices'} onClick={() => switchKind('notices')}>공지사항</button>
        <button type="button" role="tab" className="tab" aria-selected={kind === 'events'} onClick={() => switchKind('events')}>이벤트</button>
      </div>

      {draft ? (
        <Editor
          kind={kind} draft={draft} saving={saving}
          onChange={setDraft} onSave={save} onCancel={() => setDraft(null)} onDelete={remove}
        />
      ) : (
        <>
          <div className="aq-cadmin-bar">
            <p>{rows ? `${rows.filter(r => stateOf(r, now) === 'live').length}개 게시 중 · 전체 ${rows.length}개` : '불러오는 중'}</p>
            <button type="button" className="button secondary" onClick={() => void load()}>새로고침</button>
            <button type="button" className="button" onClick={() => setDraft(emptyDraft())}>{kind === 'notices' ? '새 공지 쓰기' : '새 이벤트 쓰기'}</button>
          </div>
          {loadError && (
            <div className="empty-page"><h2>목록을 불러오지 못했어요.</h2><p>{loadError}</p></div>
          )}
          {rows && !rows.length && !loadError && (
            <div className="empty-page"><h2>아직 올린 글이 없어요.</h2><p>첫 글을 써 보세요. 저장하면 바로 스튜디오에 보여요.</p></div>
          )}
          <ul className="aq-cadmin-list">
            {rows?.map(r => {
              const st = stateOf(r, now);
              const ev = kind === 'events' ? r as AdminEvent : null;
              return (
                <li key={r.id} className={`is-${st}`}>
                  <button type="button" className="aq-cadmin-row" onClick={() => setDraft(draftOf(kind, r))}>
                    <span className="aq-cadmin-row-title">
                      {kind === 'notices' && (r as AdminNotice).pinned && <em className="aq-nboard-tag">중요</em>}
                      {r.title}
                    </span>
                    <span className="aq-cadmin-row-meta">
                      <span className={`aq-cadmin-state is-${st}`}>{STATE_LABEL[st]}</span>
                      {ev && <span>{ev.starts_on}{ev.ends_on ? ` ~ ${ev.ends_on}` : ''} · {EVENT_STATUS[ev.status] ?? ''}</span>}
                      <span>게시 {kstLabel(r.published_at)}</span>
                    </span>
                  </button>
                  {st === 'live' && <Link to={viewHref(r)} className="aq-cadmin-view" target="_blank">보기</Link>}
                </li>
              );
            })}
          </ul>
        </>
      )}
    </div>
  );
}
