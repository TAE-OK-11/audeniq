import { ReadFailure } from '../components/ReadFailure';
// 문의 (/inquiries) — 담당자에게 보내는 모든 요청을 하나의 티켓 목록으로. 일반 문의(답변 대화)와
// 플랫폼 요청(아티스트 페이지 정리·유튜브 Content ID, 플랫폼 처리 단계)을 같은 목록·같은 작성 창에서 다룬다.
import { useEffect, useMemo, useRef, useState } from 'react';
import { Link, useSearchParams } from '../lib/router';
import { Modal, useModalClose } from '../components/Modal';
import { useToast } from '../components/Toast';
import { useConfirm } from '../components/Confirm';
import { niceDate } from '../lib/format';
import { todayStr, parseStamp } from '../lib/date';
import { uid } from '../lib/store';
import { api } from '../api/client';
import type { Release } from '../api/types';
import { useAsync } from '../hooks/useAsync';
import { ticketsStore, type Ticket } from '../store/tickets';
import { MOCK } from '../lib/mode';
import * as portal from '../api/portal';
import { orgPath, req } from '../api/http';
import { errorMessage } from '../api/errors';
import { refreshTickets } from '../store/portalSync';
import { INQUIRY_CHANGED_EVENT } from '../lib/liveEvents';
import { PLATFORM_NAMES } from '../lib/platformNames';
import { KINDS, STATUS_LABEL, STATUS_TONE, kindSpec, type PlatformRequest, type RequestGroup } from '../lib/platformRequests';
import { Glyph } from '../components/Glyph';
import { HelpFaq } from '../components/HelpFaq';
import { Badge, EmptyState, PageHeader, SkeletonList } from '../ui';
import { StageCard } from '../components/StageCard';
import type { Tone } from '../ui/tone';

/** 일반 문의 유형 */
const INQUIRY_CATEGORIES = ['발매·심사', '수정·테이크다운', '정산·지급', '계약·권리', '본인인증·실명인증', '계정·기타'];
const IDENTITY_CATEGORY = '본인인증·실명인증';
/** 플랫폼에 전달하는 요청 — 같은 작성 창에서 유형으로 고른다 */
const REQUEST_CATEGORIES: Record<string, RequestGroup> = { '아티스트 페이지': 'profile', '유튜브 Content ID': 'content_id' };
const GROUP_CATEGORY: Record<RequestGroup, string> = { profile: '아티스트 페이지', content_id: '유튜브 Content ID' };
const PROFILE_PLATFORMS = ['melon', 'genie', 'flo', 'bugs', 'vibe', 'spotify', 'apple', 'youtube', 'amazon', 'deezer', 'tidal'];

const inquiryTone = (status: string): Tone => (status === '답변 완료' ? 'green' : status === '종료' ? 'gray' : 'violet');

/** 대화 시각 — 오늘이면 '오후 5:45', 아니면 '10월 29일 오후 5:45' ('2026-10-29 17:45'는 읽기 어려웠다) */
function chatTime(v: string): string {
  const d = parseStamp(v);
  if (!d) return v;
  const t = d.toLocaleTimeString('ko-KR', { hour: 'numeric', minute: '2-digit' });
  const now = new Date();
  if (d.toDateString() === now.toDateString()) return t;
  const md = `${d.getMonth() + 1}월 ${d.getDate()}일`;
  return `${d.getFullYear() === now.getFullYear() ? md : `${d.getFullYear()}년 ${md}`} ${t}`;
}

/** 실서버 문의 대화 — 담당자 답변을 보고 이어서 묻거나 문의를 닫는다 */
function TicketThread({ ticket, onClosed }: { ticket: Ticket; onClosed: () => void }) {
  const toast = useToast();
  const { data, loading, error, reload } = useAsync(() => portal.fetchInquiry(ticket.id), [ticket.id]);
  const [reply, setReply] = useState('');
  const [busy, setBusy] = useState(false);
  const status = data?.ticket.status ?? ticket.status;
  const listRef = useRef<HTMLOListElement>(null);
  // 담당자가 답하면 실시간 연결로 바로 다시 받는다 (새로 고침 없이)
  useEffect(() => {
    const on = (e: Event) => {
      const id = (e as CustomEvent<{ inquiryId: string | null }>).detail?.inquiryId;
      if (!id || id === ticket.id) reload();
    };
    window.addEventListener(INQUIRY_CHANGED_EVENT, on);
    return () => window.removeEventListener(INQUIRY_CHANGED_EVENT, on);
  }, [ticket.id, reload]);
  const count = data?.messages.length ?? 0;
  useEffect(() => { const el = listRef.current; if (el) el.scrollTop = el.scrollHeight; }, [count]);

  const send = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!reply.trim() || busy) return;
    setBusy(true);
    try {
      await portal.replyInquiry(ticket.id, reply.trim());
      setReply('');
      reload();
      void refreshTickets();
    } catch (err) {
      toast(errorMessage(err, '메시지를 보내지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };

  const close = async () => {
    setBusy(true);
    try {
      await portal.closeInquiry(ticket.id);
      await refreshTickets();
      toast('문의를 종료했어요.');
      onClosed();
    } catch (err) {
      toast(errorMessage(err, '문의를 종료하지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };

  if (loading && !data) return <SkeletonList rows={2} />;
  if (error) return (
    <div className="ui-empty">
      <h2>대화를 불러오지 못했어요.</h2>
      <p>{error}</p>
      <button type="button" className="ui-btn is-soft" onClick={reload}>다시 불러오기</button>
    </div>
  );
  return (
    <>
      <ol className="aq-thread" ref={listRef} aria-live="polite">
        {(data?.messages ?? []).map(m => (
          <li key={m.id} className={m.from === 'staff' ? 'is-staff' : ''}>
            <span className="aq-thread-who">{m.from === 'staff' ? 'AUDENIQ 담당자' : '나'} · {chatTime(m.time)}</span>
            <p>{m.body}</p>
          </li>
        ))}
      </ol>
      {status !== '종료' ? (
        <form onSubmit={send} className="aq-thread-reply aq-sticky-foot">
          <label htmlFor="tReply" className="sr-only">추가 메시지</label>
          <div className="aq-thread-compose">
            <textarea id="tReply" rows={1} maxLength={4000} value={reply} onChange={e => setReply(e.target.value)} placeholder="담당자에게 더 전할 내용" />
            <button type="submit" className={`ui-btn is-primary is-icon${busy ? ' is-busy' : ''}`} aria-label="보내기" disabled={busy || !reply.trim()}>
              <Glyph name="arrow-up" size={18} />
            </button>
          </div>
          <button type="button" className="ui-btn is-text is-sm aq-thread-end" onClick={close} disabled={busy}>해결됐어요 · 문의 종료</button>
        </form>
      ) : (
        <div className="ui-callout aq-ticket-closed">종료된 문의예요. 더 궁금한 점은 새 문의로 남겨 주세요.</div>
      )}
    </>
  );
}

/** 지금 단계를 한 줄로 */
const STATUS_HELP: Record<string, string> = {
  SUBMITTED: '요청을 받았어요. 담당자가 내용을 확인하고 있어요.',
  IN_PROGRESS: '담당자가 플랫폼에 보낼 준비를 하고 있어요.',
  SENT_TO_PLATFORM: '플랫폼에 전달했어요. 반영까지 며칠에서 몇 주 걸릴 수 있어요.',
  RESOLVED: '플랫폼에 반영됐어요.',
  REJECTED: '이번 요청은 처리하지 못했어요. 아래 사유를 확인해 주세요.',
  CANCELLED: '취소한 요청이에요.',
};
const STEPS = ['SUBMITTED', 'IN_PROGRESS', 'SENT_TO_PLATFORM', 'RESOLVED'];

/** 플랫폼 요청 상세 — 요약(플랫폼·아티스트) → 진행 단계 카드(접수 → 처리 중 → 플랫폼 전달 → 완료) → 남긴 내용 */
function RequestDetail({ r, onCancel }: { r: PlatformRequest & { artist_id?: string | null }; onCancel: () => void }) {
  // 발매 상세와 같은 진행 단계 카드 — 끝난 요청(처리 불가·취소)은 멈춘 다음 칸에 그 상태를 빨간색으로
  const stopped = r.status === 'REJECTED' || r.status === 'CANCELLED';
  const reached = stopped ? Math.max(0, ...r.events.map(e => STEPS.indexOf(e.status))) : STEPS.indexOf(r.status);
  const current = stopped ? Math.min(reached + 1, STEPS.length - 1) : Math.max(0, reached);
  const labels = STEPS.map((st, i) => (stopped && i === current ? STATUS_LABEL[r.status] : STATUS_LABEL[st]));
  const notes = r.events.filter(e => e.note.trim());
  const platform = PLATFORM_NAMES[r.platform] ?? r.platform;
  return (
    <div className="aq-request-detail">
      <section className="aq-request-sum">
        <span className="aq-request-sum-icon" aria-hidden="true"><Glyph name={r.kind === 'PROFILE_IMAGE' ? 'user' : 'globe'} size={20} /></span>
        <span className="min-0">
          <strong>{platform}</strong>
          <small>{[r.artist_name, r.release_title].filter(Boolean).join(' · ')}</small>
        </span>
      </section>

      <StageCard steps={labels} current={current} complete={r.status === 'RESOLVED'} tone={stopped ? 'error' : undefined}
        note={STATUS_HELP[r.status]} label="요청 진행 단계" />

      {(r.resolution || notes.length > 0) && (
        <section className="aq-request-block">
          <h3>담당자 메모</h3>
          {r.resolution && <p>{r.resolution}</p>}
          {notes.map((e, i) => <p key={i}><span>{STATUS_LABEL[e.status] ?? e.status} · {niceDate(e.at)}</span>{e.note}</p>)}
        </section>
      )}
      {(r.platform_ref || r.urls.length > 0 || r.description) && (
        <section className="aq-request-block">
          <h3>요청 내용</h3>
          {r.platform_ref && <p><span>플랫폼 접수 번호</span>{r.platform_ref}</p>}
          {r.urls.map(u => <p key={u}><a href={u} target="_blank" rel="noopener noreferrer">{u}</a></p>)}
          {r.description && <p className="is-pre">{r.description}</p>}
        </section>
      )}
      {r.kind === 'PROFILE_IMAGE' && (
        <Link className="ui-btn is-soft is-block" to={`/artist?tab=photo${r.artist_id ? `&artist=${encodeURIComponent(r.artist_id)}` : ''}`}>
          <Glyph name="user" size={15} />아티스트 페이지에서 사진 보기
        </Link>
      )}
      {(r.status === 'SUBMITTED' || r.status === 'IN_PROGRESS') && (
        <button type="button" className="ui-btn is-block aq-danger-text aq-request-cancel" onClick={onCancel}>요청 취소</button>
      )}
    </div>
  );
}

/** '[발매 정보 수정 요청] Paper Moon' → 꼬리표와 제목으로 나눠 보여 준다 */
function splitSubject(subject: string): { tag: string; title: string } {
  const m = /^\[([^\]]{1,30})\]\s*(.*)$/.exec(subject.trim());
  return m ? { tag: m[1], title: m[2] || m[1] } : { tag: '', title: subject };
}

interface TicketPreset { category: string; releaseId: string; subject: string; body: string; kind?: string }

type Submit =
  | { type: 'inquiry'; ticket: Omit<Ticket, 'id' | 'created' | 'status'> }
  | { type: 'request'; body: Record<string, unknown>; label: string; releaseTitle: string };

function TicketForm({ onSave, preset, releases, artist }: {
  onSave: (s: Submit) => Promise<boolean>; preset?: TicketPreset | null; releases: Release[]; artist: string;
}) {
  const close = useModalClose();
  const toast = useToast();
  const [category, setCategory] = useState(preset?.category ?? INQUIRY_CATEGORIES[0]);
  const group = REQUEST_CATEGORIES[category] as RequestGroup | undefined;
  const kinds = group ? KINDS.filter(k => k.group === group) : [];
  const [kind, setKind] = useState(preset?.kind ?? '');
  const spec = group ? kindSpec(kinds.some(k => k.kind === kind) ? kind : kinds[0].kind)! : null;
  const [platform, setPlatform] = useState('melon');
  const [releaseId, setReleaseId] = useState(preset?.releaseId ?? '');
  const [name, setName] = useState(artist);
  const [urls, setUrls] = useState<string[]>([]);
  const [subject, setSubject] = useState(preset?.subject ?? '');
  const [body, setBody] = useState(preset?.body ?? '');
  const [busy, setBusy] = useState(false);
  useEffect(() => { setUrls([]); }, [spec?.kind]);
  // 아티스트 이름은 처음 한 번만 채운다 (사용자가 지운 칸을 다시 채우지 않도록)
  const seeded = useRef(!!artist);
  useEffect(() => { if (!seeded.current && artist) { seeded.current = true; setName(artist); } }, [artist]);
  // 발매 목록을 받은 뒤 제목을 채운다
  useEffect(() => {
    if (!preset?.releaseId || subject !== preset.subject) return;
    const title = releases.find(r => r.id === preset.releaseId)?.title;
    if (title && preset.subject) setSubject(`${preset.subject} ${title}`);
  }, [releases]); // eslint-disable-line react-hooks/exhaustive-deps

  const submitted = releases.filter(r => r.status !== 'draft');
  const releaseChoices = spec ? submitted : releases;
  const releaseMode = spec ? spec.release : 'optional';

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busy) return;
    const rel = releases.find(r => r.id === releaseId);
    let s: Submit;
    if (spec) {
      const list = urls.map(u => u.trim()).filter(Boolean);
      if (!name.trim()) { toast('아티스트명을 적어 주세요.', 'error'); return; }
      if (spec.release === 'required' && !releaseId) { toast('발매를 골라 주세요.', 'error'); return; }
      if (list.length < spec.minUrls) { toast(`${spec.urls[0]}를 적어 주세요.`, 'error'); return; }
      if (spec.description === 'required' && body.trim().length < 10) { toast('상황을 10자 이상 적어 주세요.', 'error'); return; }
      s = {
        type: 'request', label: spec.label, releaseTitle: rel?.title ?? '',
        body: {
          kind: spec.kind, platform: spec.youtube ? 'youtube' : platform, artist_name: name.trim(),
          ...(releaseId ? { release_id: releaseId } : {}), urls: list, description: body.trim(),
        },
      };
    } else {
      if (!subject.trim() || !body.trim()) { toast('제목과 내용을 입력해 주세요.'); return; }
      s = { type: 'inquiry', ticket: { category, releaseId, releaseTitle: rel?.title ?? '', subject: subject.trim(), body: body.trim() } };
    }
    // 보내지 못했으면 작성한 내용이 남도록 창을 닫지 않는다
    setBusy(true);
    const ok = await onSave(s);
    setBusy(false);
    if (ok) close();
  };

  return (
    <form id="ticketForm" onSubmit={submit}>
      <p className="small muted">{spec ? '플랫폼에 전달할 요청이에요. 담당자가 확인해 플랫폼에 접수하고 결과까지 알려 드려요.' : '문의 내용을 작성하고 필요한 발매를 연결해 주세요.'}</p>
      <div className="form-grid">
        <div className="ui-field">
          <label htmlFor="tCategory">유형</label>
          <select id="tCategory" value={category} onChange={e => { setCategory(e.target.value); setKind(''); }}>
            {INQUIRY_CATEGORIES.map(c => <option key={c}>{c}</option>)}
            {Object.keys(REQUEST_CATEGORIES).map(c => <option key={c}>{c}</option>)}
          </select>
        </div>
        {releaseMode !== 'none' && (
          <div className="ui-field">
            <label htmlFor="tRelease">관련 발매</label>
            <select id="tRelease" value={releaseId} onChange={e => setReleaseId(e.target.value)}>
              <option value="">{releaseMode === 'required' ? '발매 선택' : '선택 안 함'}</option>
              {releaseChoices.map(r => <option key={r.id} value={r.id}>{r.title}</option>)}
            </select>
          </div>
        )}
      </div>

      {spec ? (
        <div className="aq-reveal" key={category}>
          <div className="ui-field">
            <label htmlFor="tKind">요청 내용</label>
            <select id="tKind" value={spec.kind} onChange={e => setKind(e.target.value)}>
              {kinds.map(k => <option key={k.kind} value={k.kind}>{k.label}</option>)}
            </select>
            <small className="ui-field-help">{spec.help}</small>
          </div>
          <div className="form-grid">
            {!spec.youtube && (
              <div className="ui-field">
                <label htmlFor="tPlatform">플랫폼</label>
                <select id="tPlatform" value={platform} onChange={e => setPlatform(e.target.value)}>
                  {PROFILE_PLATFORMS.map(p => <option key={p} value={p}>{PLATFORM_NAMES[p] ?? p}</option>)}
                </select>
              </div>
            )}
            <div className="ui-field">
              <label htmlFor="tArtist">아티스트명</label>
              <input id="tArtist" maxLength={200} value={name} onChange={e => setName(e.target.value)} />
            </div>
          </div>
          {spec.urls.map((label, i) => (
            <div key={`${spec.kind}-${i}`} className="ui-field">
              <label htmlFor={`tUrl${i}`}>{label}</label>
              <input id={`tUrl${i}`} type="url" inputMode="url" maxLength={500} placeholder="https://"
                value={urls[i] ?? ''} onChange={e => setUrls(u => { const n = [...u]; n[i] = e.target.value; return n; })} />
            </div>
          ))}
          <div className="ui-field">
            <label htmlFor="tBody">상황 설명{spec.description === 'required' ? '' : <span className="aq-optional">선택</span>}</label>
            <textarea id="tBody" maxLength={2000} rows={4} value={body} onChange={e => setBody(e.target.value)} />
          </div>
        </div>
      ) : (
        <>
          {category === IDENTITY_CATEGORY && (
            <div className="ui-callout aq-inquiry-identity">
              휴대폰·PASS로 본인인증이 안 되면 고객센터에서 신분증 원본을 보고 실명인증을 도와드려요.
              만 19세 미만은 청소년증이나, 법정대리인과 함께 가족관계증명서로 확인할 수 있어요.
              <b> 신분증 사진이나 주민등록번호는 문의에 올리지 마세요.</b>
            </div>
          )}
          <div className="ui-field">
            <label htmlFor="tSubject">제목</label>
            <input id="tSubject" maxLength={180} required placeholder="문의 제목" value={subject} onChange={e => setSubject(e.target.value)} />
          </div>
          <div className="ui-field">
            <label htmlFor="tBody">내용</label>
            <textarea id="tBody" maxLength={4000} rows={6} required placeholder="상황과 확인이 필요한 내용을 자세히 입력해 주세요." value={body} onChange={e => setBody(e.target.value)} />
            <p className="ui-field-help aq-counter">{body.length} / 4000</p>
          </div>
        </>
      )}
      <div className="aq-sticky-foot">
        <button className={`ui-btn is-primary studio-submit-wide${busy ? ' is-busy' : ''}`} type="submit" disabled={busy}>{busy ? '보내는 중' : '보내기'}</button>
      </div>
    </form>
  );
}

/** 목록 한 줄 — 문의와 플랫폼 요청을 같은 모양으로 */
interface Row { id: string; type: 'inquiry' | 'request'; title: string; meta: string[]; created: string; tone: Tone; status: string }

export function Inquiries() {
  const toast = useToast();
  const confirm = useConfirm();
  const tickets = ticketsStore.use();
  const [params, setParams] = useSearchParams();
  const { data: releases = [] } = useAsync(() => api.listReleases(), [], { key: 'releases' });
  const artist = useMemo(() => releases.find(r => r.artist)?.artist ?? '', [releases]);

  // 플랫폼 요청 (아티스트 페이지·Content ID)
  const [requests, setRequests] = useState<PlatformRequest[] | null>(MOCK ? [] : null);
  const [requestsFailed, setRequestsFailed] = useState(false);
  const loadRequests = async () => {
    if (MOCK) return;
    try { setRequests((await req<{ items: PlatformRequest[] }>(orgPath('/platform-requests'))).items); setRequestsFailed(false); }
    catch { setRequestsFailed(true); }
  };
  useEffect(() => { void loadRequests(); }, []);

  // 다른 화면에서 넘어온 작성 요청: 신청 취소·정보 수정(new=cancel|edit), 플랫폼 요청(new=request&group=)
  const newKind = params.get('new');
  const presetRelease = params.get('release') ?? '';
  const [showForm, setShowForm] = useState(!!newKind);
  const [preset] = useState<TicketPreset | null>(() => {
    if (newKind === 'edit') return { category: INQUIRY_CATEGORIES[1], releaseId: presetRelease, subject: '[발매 정보 수정 요청]', body: '접수한 발매 정보를 아래와 같이 수정해 주세요.\n\n수정할 내용: ' };
    if (newKind === 'identity') return { category: IDENTITY_CATEGORY, releaseId: '', subject: '[실명인증 요청]', body: '휴대폰·PASS 본인인증이 되지 않아 고객센터 실명인증을 받고 싶어요.\n\n안 되는 이유(예: 본인 명의 휴대폰 없음, 미성년자): \n방문 또는 연락 가능한 시간: ' };
    if (newKind === 'cancel') return { category: INQUIRY_CATEGORIES[0], releaseId: presetRelease, subject: '[발매 취소 요청]', body: '이 발매 신청을 취소해 주세요.\n\n취소 사유: ' };
    if (newKind === 'request') {
      const g: RequestGroup = params.get('group') === 'content_id' ? 'content_id' : 'profile';
      return { category: GROUP_CATEGORY[g], releaseId: presetRelease, subject: '', body: '', kind: params.get('kind') ?? '' };
    }
    return null;
  });
  // 알림의 링크 (/inquiries?open=<id>)로 오면 그 티켓을 바로 연다
  const [openId, setOpenId] = useState<string | null>(() => params.get('open'));
  useEffect(() => {
    if (!newKind && !params.get('open')) return;
    if (params.get('open')) setOpenId(params.get('open'));
    setParams({}, { replace: true });
  }, [params]); // eslint-disable-line react-hooks/exhaustive-deps

  const rows = useMemo<Row[]>(() => [
    ...tickets.map(t => ({
      id: t.id, type: 'inquiry' as const, title: splitSubject(t.subject).title, created: t.created,
      meta: [splitSubject(t.subject).tag || t.category, niceDate(t.created), t.releaseTitle].filter(Boolean),
      tone: inquiryTone(t.status), status: t.status,
    })),
    ...(requests ?? []).map(r => {
      // 같은 요청을 여러 플랫폼에 보내면 제목이 같아진다 — 어느 플랫폼인지 먼저 보이게
      return {
        id: r.id, type: 'request' as const, title: kindSpec(r.kind)?.label ?? r.kind, created: r.created_at,
        meta: [PLATFORM_NAMES[r.platform] ?? r.platform, r.release_title, niceDate(r.created_at)].filter(Boolean) as string[],
        tone: (STATUS_TONE[r.status] ?? 'gray') as Tone, status: STATUS_LABEL[r.status] ?? r.status,
      };
    }),
  ].sort((a, b) => b.created.localeCompare(a.created)), [tickets, requests]);

  const openTicket = tickets.find(t => t.id === openId) ?? null;
  const openRequest = requests?.find(r => r.id === openId) ?? null;

  const removeTicket = async (t: Ticket) => {
    const ok = await confirm({ title: '문의 기록을 삭제할까요?', message: '삭제하면 되돌릴 수 없어요.', confirmLabel: '삭제', danger: true });
    if (!ok) return;
    ticketsStore.set(ts => ts.filter(x => x.id !== t.id));
    setOpenId(null);
    toast('문의 기록을 삭제했어요.');
  };

  const cancelRequest = async (r: PlatformRequest) => {
    if (!(await confirm({ title: '요청을 취소할까요?', message: '플랫폼에 전달되기 전까지만 취소할 수 있어요.', confirmLabel: '요청 취소', danger: true }))) return;
    try {
      if (!MOCK) await req(orgPath(`/platform-requests/${encodeURIComponent(r.id)}/cancel`), { method: 'POST', body: {} });
      else setRequests(rs => (rs ?? []).map(x => (x.id === r.id ? { ...x, status: 'CANCELLED' } : x)));
      setOpenId(null);
      void loadRequests();
    } catch (e) { toast(errorMessage(e), 'error'); }
  };

  const save = async (s: Submit): Promise<boolean> => {
    if (s.type === 'request') {
      try {
        if (!MOCK) {
          await req(orgPath('/platform-requests'), { method: 'POST', body: s.body });
          await loadRequests();
        } else {
          const now = new Date().toISOString();
          setRequests(rs => [{
            id: uid('pr'), kind: String(s.body.kind), platform: String(s.body.platform), release_id: (s.body.release_id as string) ?? null,
            release_title: s.releaseTitle || null, artist_name: String(s.body.artist_name), urls: s.body.urls as string[],
            description: String(s.body.description), status: 'SUBMITTED', platform_ref: '', resolution: '', created_at: now, updated_at: now,
            events: [{ status: 'SUBMITTED', note: '', by_staff: false, at: now }],
          }, ...(rs ?? [])]);
        }
        toast('요청을 접수했어요. 진행 상황은 문의 목록과 알림으로 알려 드려요.', 'success');
        return true;
      } catch (err) {
        toast(errorMessage(err), 'error');
        return false;
      }
    }
    if (!MOCK) {
      try {
        await portal.createInquiry(s.ticket);
        await refreshTickets();
        toast('문의를 보냈어요. 답변이 오면 알림으로 알려 드릴게요.', 'success');
        return true;
      } catch (err) {
        toast(errorMessage(err, '문의를 보내지 못했어요. 잠시 후 다시 시도해 주세요.'), 'error');
        return false;
      }
    }
    ticketsStore.set(ts => [...ts, { ...s.ticket, id: uid('q'), created: todayStr(), status: '답변 대기' }]);
    toast('문의 내용을 저장했어요.');
    return true;
  };

  const listLoading = !requestsFailed && requests === null && !tickets.length;

  return (
    <div id="view-inquiries" className="view">
      <PageHeader className="aq-head-inline"
        eyebrow="계정·지원"
        title="문의"
        sub="발매·정산·계약 문의와 플랫폼 요청을 남기고 답변을 받아요."
        actions={<button type="button" className="ui-btn is-primary" onClick={() => setShowForm(true)}><Glyph name="plus" size={14} />새 문의</button>}
      />

      {requestsFailed && <ReadFailure label="플랫폼 요청" retry={loadRequests} />}
      {/* 내 문의가 있으면 답변 상태가 먼저 — 자주 묻는 질문 다섯 줄 아래에 묻혀 있었다. 없으면 질문부터 보여 스스로 답을 찾게 */}
      {!listLoading && !requestsFailed && !rows.length && <HelpFaq />}

      {listLoading ? (
        <SkeletonList rows={3} />
      ) : rows.length ? (
        <section className="aq-inq-mine" aria-labelledby="inqMineTitle">
        <h2 id="inqMineTitle">내 문의</h2>
        <div className="aq-catalog-cards aq-stagger">
          {rows.map(t => (
            <button key={t.type + t.id} type="button" className="ui-row aq-ticket-card" onClick={() => setOpenId(t.id)}>
              <span className="aq-ticket-icon" aria-hidden="true"><Glyph name={t.type === 'request' ? 'globe' : 'mail'} size={18} /></span>
              <span className="min-0">
                <span className="ui-row-title">{t.title}</span>
                <span className="ui-row-meta">{t.meta.join(' · ')}</span>
              </span>
              <Badge tone={t.tone}>{t.status}</Badge>
            </button>
          ))}
        </div>
        </section>
      ) : requestsFailed ? null : (
        <EmptyState
          icon={<Glyph name="inquiry" size={22} />} title="작성한 문의가 없어요" as="h2"
          actions={<button type="button" className="ui-btn is-primary" onClick={() => setShowForm(true)}>문의 작성하기</button>}
        >발매·정산·권리 관련 문의와 아티스트 페이지·Content ID 요청을 보내고 진행 상황을 볼 수 있어요.</EmptyState>
      )}

      {rows.length > 0 && <HelpFaq />}

      {showForm && (
        <Modal title="새 문의" onClose={() => setShowForm(false)} dismissible={false}>
          <TicketForm preset={preset} releases={releases} artist={artist} onSave={save} />
        </Modal>
      )}

      {openRequest && (
        <Modal title={kindSpec(openRequest.kind)?.label ?? '플랫폼 요청'} onClose={() => setOpenId(null)}>
          <RequestDetail r={openRequest} onCancel={() => void cancelRequest(openRequest)} />
        </Modal>
      )}

      {openTicket && (
        <Modal title={splitSubject(openTicket.subject).title} onClose={() => setOpenId(null)} modalClass={MOCK ? undefined : 'aq-chat-mode'}>
          <p className="aq-ticket-meta">
            {splitSubject(openTicket.subject).tag && <Badge tone="blue">{splitSubject(openTicket.subject).tag}</Badge>}
            <span>{[openTicket.category, niceDate(openTicket.created), openTicket.releaseTitle].filter(Boolean).join(' · ')}</span>
          </p>
          {!MOCK ? <TicketThread ticket={openTicket} onClosed={() => setOpenId(null)} /> : (<>
          {/* 체험 모드도 실서버와 같은 대화 모양 — 내 문의는 오른쪽, 담당자 답변은 왼쪽 말풍선 */}
          <ol className="aq-thread">
            <li><span className="aq-thread-who">나 · {niceDate(openTicket.created)}</span><p>{openTicket.body}</p></li>
            <li className="is-staff">
              <span className="aq-thread-who">AUDENIQ 담당자</span>
              <p>{openTicket.status === '답변 완료' ? '답변을 등록했어요. 자세한 내용은 가입한 이메일로 안내해 드렸어요.' : '문의를 받았어요. 확인 후 가입한 이메일로 답변을 드릴게요.'}</p>
            </li>
          </ol>
          <div className="row-actions aq-ticket-closed">
            <button type="button" className="ui-btn is-text aq-danger-text" onClick={() => removeTicket(openTicket)}>문의 기록 삭제</button>
          </div>
          </>)}
        </Modal>
      )}
    </div>
  );
}
