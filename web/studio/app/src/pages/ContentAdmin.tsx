// 공지·이벤트 관리 — D1(CONTENT_DB)에 글을 쓰고 고치고 내린다.
// 로그인 대신 Worker 시크릿 CONTENT_ADMIN_TOKEN으로 인증한다 (토큰은 이 탭의 sessionStorage에만 보관).
import { useCallback, useEffect, useState, type FormEvent } from 'react';
import { Link } from '../lib/router';
import { useToast } from '../components/Toast';
import { useConfirm } from '../components/Confirm';
import { errorMessage } from '../api/errors';
import {
  contentAdmin, type AdminEvent, type AdminMaintenance, type AdminNotice, type ContentKind, type EventInput,
  type MaintenanceInput, type NoticeInput,
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

type Row = AdminNotice | AdminEvent | AdminMaintenance;
type State = 'live' | 'scheduled' | 'deleted' | 'active' | 'ended';
const isMaint = (r: Row): r is AdminMaintenance => 'starts_at' in r;
function stateOf(r: Row, now: string): State {
  if (r.deleted_at) return 'deleted';
  if (isMaint(r)) {
    if (r.ends_at <= now) return 'ended';
    if (r.starts_at <= now) return 'active';
  }
  return r.published_at > now ? 'scheduled' : 'live';
}
const STATE_LABEL: Record<State, string> = { live: '게시 중', scheduled: '예약', deleted: '내림', active: '점검 중', ended: '종료' };
const MAINT_STATE_LABEL: Partial<Record<State, string>> = { live: '예고 중', scheduled: '예고 전' };
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
  /** 서버 점검 시작·예상 종료 (datetime-local, KST) */
  maintStart: string;
  maintEnd: string;
  maintKind: 'scheduled' | 'emergency';
  endUnknown: boolean;
  deleted: boolean;
}

const emptyDraft = (): Draft => ({
  id: null, title: '', body: '', pinned: false, publishedAt: nowKstInput(),
  summary: '', place: '', startsOn: nowKstInput().slice(0, 10), endsOn: '', linkUrl: '', deleted: false,
  maintStart: '', maintEnd: '', maintKind: 'scheduled', endUnknown: false,
});

function draftOf(kind: ContentKind, r: Row): Draft {
  const base = { ...emptyDraft(), id: r.id, title: r.title, body: r.body, publishedAt: toKstInput(r.published_at), deleted: !!r.deleted_at };
  if (kind === 'notices') return { ...base, pinned: !!(r as AdminNotice).pinned };
  if (kind === 'maintenance') {
    const m = r as AdminMaintenance;
    return {
      ...base, maintStart: toKstInput(m.starts_at), maintEnd: toKstInput(m.ends_at),
      maintKind: m.kind === 'emergency' ? 'emergency' : 'scheduled', endUnknown: !!m.end_unknown,
    };
  }
  const e = r as AdminEvent;
  return { ...base, summary: e.summary, place: e.place, startsOn: e.starts_on, endsOn: e.ends_on ?? '', linkUrl: e.link_url ?? '' };
}

function inputOf(kind: ContentKind, d: Draft): NoticeInput | EventInput | MaintenanceInput {
  const published_at = fromKstInput(d.publishedAt);
  if (kind === 'notices') return { title: d.title, body: d.body, pinned: d.pinned, published_at };
  if (kind === 'maintenance') {
    return {
      title: d.title, body: d.body, starts_at: fromKstInput(d.maintStart), ends_at: fromKstInput(d.maintEnd), published_at,
      kind: d.maintKind, end_unknown: d.endUnknown,
    };
  }
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
  const maint = kind === 'maintenance';
  const scheduled = fromKstInput(draft.publishedAt) > new Date().toISOString();
  const heading = maint ? (draft.id ? '점검 일정 수정' : '새 점검 일정')
    : draft.id ? (notice ? '공지 수정' : '이벤트 수정') : (notice ? '새 공지' : '새 이벤트');
  return (
    <form className="aq-cadmin-editor" onSubmit={e => { e.preventDefault(); onSave(); }}>
      <div className="aq-cadmin-editor-head">
        <h2>{heading}</h2>
        {draft.deleted && <span className="status-chip">내린 글 · 저장하면 다시 게시돼요</span>}
      </div>
      <div className="field">
        <label htmlFor="caTitle">제목 <span className="required">*</span></label>
        <input
          id="caTitle" required maxLength={200} value={draft.title} onChange={e => set('title', e.target.value)}
          placeholder={maint ? '예: 서버 업데이트 점검' : undefined}
        />
      </div>
      {maint && (
        <div className="aq-cadmin-grid">
          <div className="field">
            <label htmlFor="caMaintStart">점검 시작 (한국 시간) <span className="required">*</span></label>
            <input id="caMaintStart" type="datetime-local" required value={draft.maintStart} onChange={e => set('maintStart', e.target.value)} />
          </div>
          <div className="field">
            <label htmlFor="caMaintEnd">예상 종료 (한국 시간) <span className="required">*</span></label>
            <input id="caMaintEnd" type="datetime-local" required min={draft.maintStart} value={draft.maintEnd} onChange={e => set('maintEnd', e.target.value)} />
          </div>
          <label className="aq-cadmin-check">
            <input type="checkbox" checked={draft.endUnknown} onChange={e => set('endUnknown', e.target.checked)} />
            <span>스튜디오에 '종료 시각 미정'으로 표시</span>
          </label>
        </div>
      )}
      {kind === 'events' && (
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
        <label htmlFor="caBody">{maint ? '안내 문구' : '본문'}</label>
        <textarea
          id="caBody" rows={maint ? 5 : 12} maxLength={maint ? 2000 : 20000} value={draft.body} onChange={e => set('body', e.target.value)}
          placeholder={maint ? '예: 더 안정적인 서비스를 위해 서버를 업데이트해요. 점검 중에는 로그인과 발매 접수를 할 수 없어요.' : undefined}
        />
        <p className="help">줄바꿈은 그대로 보여요. {draft.body.length.toLocaleString()} / {maint ? '2,000' : '20,000'}자</p>
      </div>
      <div className="aq-cadmin-grid">
        <div className="field">
          <label htmlFor="caPublished">{maint ? '예고 공개 시각 (한국 시간)' : '게시 시각 (한국 시간)'}</label>
          <input id="caPublished" type="datetime-local" required value={draft.publishedAt} onChange={e => set('publishedAt', e.target.value)} />
          <p className="help">
            {maint
              ? '점검 72시간 전부터 스튜디오 상단에 예고가 뜨고, 시작 시각이 되면 점검 화면으로 바뀌어요.'
              : scheduled ? '이 시각이 되면 자동으로 보여요.' : '저장하면 바로 보여요.'}
          </p>
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

const EMERGENCY_DURATIONS: { label: string; minutes: number | null }[] = [
  { label: '30분', minutes: 30 }, { label: '1시간', minutes: 60 }, { label: '2시간', minutes: 120 },
  { label: '4시간', minutes: 240 }, { label: '미정', minutes: null },
];
const EMERGENCY_TEXT = '서비스를 안정적으로 되돌리기 위해 긴급 점검을 하고 있어요. 점검 중에는 로그인과 발매 접수를 할 수 없어요. 불편을 드려 죄송해요.';
const isoIn = (ms: number) => new Date(Date.now() + ms).toISOString().replace(/\.\d{3}Z$/, 'Z');
const minutesSince = (iso: string) => Math.max(0, Math.round((Date.now() - Date.parse(iso)) / 60_000));

/** 서버 점검 탭 맨 위: 지금 상태 + 긴급 점검 시작·연장·종료 */
function EmergencyPanel({ token, rows, now, onChanged }: {
  token: string; rows: AdminMaintenance[]; now: string; onChanged: () => Promise<void>;
}) {
  const toast = useToast();
  const confirm = useConfirm();
  const [open, setOpen] = useState(false);
  const [body, setBody] = useState(EMERGENCY_TEXT);
  const [minutes, setMinutes] = useState<number | null>(60);
  const [busy, setBusy] = useState(false);
  const active = rows.find(r => stateOf(r, now) === 'active') ?? null;

  const run = async (work: () => Promise<unknown>, done: string) => {
    setBusy(true);
    try {
      await work();
      toast(done, 'success');
      setOpen(false);
      await onChanged();
    } catch (e) {
      toast(errorMessage(e, '처리하지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };

  const start = async () => {
    const ok = await confirm({
      title: '지금 긴급 점검을 시작할까요?',
      message: '스튜디오를 쓰는 모든 사용자에게 바로 점검 화면이 뜨고, 백엔드 API 요청이 막혀요.',
      confirmLabel: '긴급 점검 시작',
      danger: true,
    });
    if (!ok) return;
    const unknown = minutes === null;
    await run(() => contentAdmin.create(token, 'maintenance', {
      title: '긴급 서버 점검', body: body.trim(), kind: 'emergency', end_unknown: unknown,
      starts_at: isoIn(-1000), ends_at: isoIn((unknown ? 12 * 60 : minutes) * 60_000), published_at: isoIn(-1000),
    }), '긴급 점검을 시작했어요. 스튜디오에 바로 점검 화면이 떠요.');
  };

  const update = (w: AdminMaintenance, patch: Partial<MaintenanceInput>, done: string) => run(() => contentAdmin.update(token, 'maintenance', w.id, {
    title: w.title, body: w.body, starts_at: w.starts_at, ends_at: w.ends_at, published_at: w.published_at,
    kind: w.kind ?? 'scheduled', end_unknown: !!w.end_unknown, ...patch,
  }), done);

  const finish = async (w: AdminMaintenance) => {
    const ok = await confirm({ title: '점검을 종료할까요?', message: '스튜디오가 바로 다시 열리고 API 요청도 통과돼요.', confirmLabel: '점검 종료' });
    if (!ok) return;
    const endAt = isoIn(0) > w.starts_at ? isoIn(0) : isoIn(60_000);
    await update(w, { ends_at: endAt, end_unknown: false }, '점검을 종료했어요. 스튜디오가 다시 열려요.');
  };

  if (active) {
    const base = Math.max(Date.parse(active.ends_at), Date.now());
    return (
      <section className="aq-cadmin-emergency is-active" aria-live="polite">
        <div className="aq-cadmin-emergency-head">
          <span className="aq-cadmin-pulse" aria-hidden="true" />
          <strong>{active.kind === 'emergency' ? '긴급 점검 중' : '점검 중'}</strong>
          <span>{minutesSince(active.starts_at)}분째 · {active.end_unknown ? '종료 시각 미정' : `${kstLabel(active.ends_at)} 종료 예정`}</span>
        </div>
        <p>스튜디오 전체에 점검 화면이 떠 있고, 백엔드 API 요청은 503으로 막혀 있어요.</p>
        <div className="aq-cadmin-emergency-actions">
          <button type="button" className="button secondary" disabled={busy}
            onClick={() => void update(active, { ends_at: new Date(base + 30 * 60_000).toISOString().replace(/\.\d{3}Z$/, 'Z'), end_unknown: false }, '30분 연장했어요.')}>
            30분 연장
          </button>
          <button type="button" className="button" disabled={busy} onClick={() => void finish(active)}>점검 종료</button>
        </div>
      </section>
    );
  }

  return (
    <section className="aq-cadmin-emergency">
      <div className="aq-cadmin-emergency-head">
        <span className="aq-cadmin-dot" aria-hidden="true" />
        <strong>정상 운영 중</strong>
        <span>진행 중인 점검이 없어요</span>
      </div>
      {!open ? (
        <div className="aq-cadmin-emergency-actions">
          <button type="button" className="button aq-cadmin-danger-fill" onClick={() => setOpen(true)}>긴급 점검 시작</button>
        </div>
      ) : (
        <div className="aq-cadmin-emergency-form">
          <div className="field">
            <label htmlFor="caEmBody">사용자에게 보일 안내</label>
            <textarea id="caEmBody" rows={3} maxLength={2000} value={body} onChange={e => setBody(e.target.value)} />
          </div>
          <div className="field">
            <span className="aq-cadmin-label">예상 소요</span>
            <div className="aq-cadmin-chips" role="radiogroup" aria-label="예상 소요">
              {EMERGENCY_DURATIONS.map(d => (
                <button key={d.label} type="button" role="radio" aria-checked={minutes === d.minutes}
                  className={`aq-cadmin-chip${minutes === d.minutes ? ' is-on' : ''}`} onClick={() => setMinutes(d.minutes)}>
                  {d.label}
                </button>
              ))}
            </div>
          </div>
          <div className="aq-cadmin-emergency-actions">
            <button type="button" className="button secondary" disabled={busy} onClick={() => setOpen(false)}>취소</button>
            <button type="button" className="button aq-cadmin-danger-fill" disabled={busy} onClick={() => void start()}>
              {busy ? '시작하는 중' : '지금 시작'}
            </button>
          </div>
        </div>
      )}
    </section>
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
    if (kind === 'maintenance') {
      const m = input as MaintenanceInput;
      if (!m.starts_at || !m.ends_at || m.ends_at <= m.starts_at) { toast('점검 시작·종료 시각을 확인해 주세요. (종료는 시작 이후)', 'error'); return; }
    }
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
        <button type="button" role="tab" className="tab" aria-selected={kind === 'maintenance'} onClick={() => switchKind('maintenance')}>서버 점검</button>
      </div>

      {draft ? (
        <Editor
          kind={kind} draft={draft} saving={saving}
          onChange={setDraft} onSave={save} onCancel={() => setDraft(null)} onDelete={remove}
        />
      ) : (
        <>
          {kind === 'maintenance' && rows && (
            <EmergencyPanel token={token} rows={rows as AdminMaintenance[]} now={now} onChanged={load} />
          )}
          <div className="aq-cadmin-bar">
            <p>{rows ? `${rows.filter(r => ['live', 'active'].includes(stateOf(r, now))).length}개 ${kind === 'maintenance' ? '예고·진행 중' : '게시 중'} · 전체 ${rows.length}개` : '불러오는 중'}</p>
            <button type="button" className="button secondary" onClick={() => void load()}>새로고침</button>
            <button type="button" className="button" onClick={() => setDraft(emptyDraft())}>{kind === 'notices' ? '새 공지 쓰기' : kind === 'events' ? '새 이벤트 쓰기' : '점검 일정 추가'}</button>
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
              const mt = isMaint(r) ? r : null;
              const label = (mt && MAINT_STATE_LABEL[st]) || STATE_LABEL[st];
              return (
                <li key={r.id} className={`is-${st}`}>
                  <button type="button" className="aq-cadmin-row" onClick={() => setDraft(draftOf(kind, r))}>
                    <span className="aq-cadmin-row-title">
                      {kind === 'notices' && (r as AdminNotice).pinned && <em className="aq-nboard-tag">중요</em>}
                      {r.title}
                    </span>
                    <span className="aq-cadmin-row-meta">
                      <span className={`aq-cadmin-state is-${st}`}>{label}</span>
                      {ev && <span>{ev.starts_on}{ev.ends_on ? ` ~ ${ev.ends_on}` : ''} · {EVENT_STATUS[ev.status] ?? ''}</span>}
                      {mt ? <span>점검 {kstLabel(mt.starts_at)} ~ {kstLabel(mt.ends_at).slice(-5)}</span> : <span>게시 {kstLabel(r.published_at)}</span>}
                    </span>
                  </button>
                  {st === 'live' && !mt && <Link to={viewHref(r)} className="aq-cadmin-view" target="_blank">보기</Link>}
                </li>
              );
            })}
          </ul>
        </>
      )}
    </div>
  );
}
