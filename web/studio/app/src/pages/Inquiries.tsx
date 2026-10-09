import { useEffect, useRef, useState } from 'react';
import { useSearchParams } from '../lib/router';
import { Modal, useModalClose } from '../components/Modal';
import { useToast } from '../components/Toast';
import { useConfirm } from '../components/Confirm';
import { niceDate } from '../lib/format';
import { todayStr } from '../lib/date';
import { uid } from '../lib/store';
import { api } from '../api/client';
import { useAsync } from '../hooks/useAsync';
import { ticketsStore, type Ticket } from '../store/tickets';
import { MOCK } from '../lib/mode';
import * as portal from '../api/portal';
import { errorMessage } from '../api/errors';
import { refreshTickets } from '../store/portalSync';
import { INQUIRY_CHANGED_EVENT } from '../lib/liveEvents';
import { Glyph } from '../components/Glyph';
import { Badge } from '../ui';

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
      toast(errorMessage(err, '메시지를 보내지 못했어요.'));
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
      toast(errorMessage(err, '문의를 종료하지 못했어요.'));
    } finally {
      setBusy(false);
    }
  };

  if (loading && !data) return <p className="small muted">대화를 불러오는 중이에요…</p>;
  if (error) return <button type="button" className="ui-btn is-soft" onClick={reload}>다시 불러오기</button>;
  return (
    <>
      <ol className="aq-thread" ref={listRef} aria-live="polite">
        {(data?.messages ?? []).map(m => (
          <li key={m.id} className={m.from === 'staff' ? 'is-staff' : ''}>
            <span className="aq-thread-who">{m.from === 'staff' ? 'AUDENIQ 담당자' : '나'} · {m.time}</span>
            <p>{m.body}</p>
          </li>
        ))}
      </ol>
      {status !== '종료' ? (
        <form onSubmit={send} className="aq-thread-reply aq-sticky-foot">
          <label htmlFor="tReply" className="sr-only">추가 메시지</label>
          <textarea id="tReply" rows={3} maxLength={4000} value={reply} onChange={e => setReply(e.target.value)} placeholder="추가로 전할 내용을 입력해 주세요" />
          <div className="row-actions">
            <button type="button" className="ui-btn is-soft" onClick={close} disabled={busy}>문의 종료</button>
            <button type="submit" className="ui-btn is-primary" disabled={busy || !reply.trim()}>보내기</button>
          </div>
        </form>
      ) : (
        <div className="ui-callout" style={{ marginTop: 16 }}>종료된 문의예요. 더 궁금한 점은 새 문의로 남겨 주세요.</div>
      )}
    </>
  );
}

const CATEGORIES = ['발매·심사', '수정·테이크다운', '정산·지급', '계약·권리', '계정·기타'];

/** 발매 상세의 ‘문의로 취소 요청’ — 내용을 미리 채워 두어 누르기만 하면 되게 */
interface TicketPreset { category: string; releaseId: string; subject: string; body: string }

function TicketForm({ onSave, preset }: { onSave: (t: Omit<Ticket, 'id' | 'created' | 'status'>) => void; preset?: TicketPreset | null }) {
  const close = useModalClose();
  const toast = useToast();
  const { data: releases = [] } = useAsync(() => api.listReleases(), []);
  const [category, setCategory] = useState(preset?.category ?? CATEGORIES[0]);
  const [releaseId, setReleaseId] = useState(preset?.releaseId ?? '');
  const [subject, setSubject] = useState(preset?.subject ?? '');
  const [body, setBody] = useState(preset?.body ?? '');
  // 발매 목록을 받은 뒤 제목을 채운다
  useEffect(() => {
    if (!preset?.releaseId || subject !== preset.subject) return;
    const title = releases.find(r => r.id === preset.releaseId)?.title;
    if (title) setSubject(`[발매 취소 요청] ${title}`);
  }, [releases]); // eslint-disable-line react-hooks/exhaustive-deps

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!subject.trim() || !body.trim()) { toast('제목과 내용을 입력해 주세요.'); return; }
    const rel = releases.find(r => r.id === releaseId);
    onSave({ category, releaseId, releaseTitle: rel?.title ?? '', subject: subject.trim(), body: body.trim() });
    close();
  };

  return (
    <form id="ticketForm" onSubmit={submit}>
      <p className="small muted">문의 내용을 작성하고 필요한 발매를 연결해 주세요.</p>
      <div className="form-grid">
        <div className="ui-field">
          <label htmlFor="tCategory">문의 유형</label>
          <select id="tCategory" value={category} onChange={e => setCategory(e.target.value)}>
            {CATEGORIES.map(c => <option key={c}>{c}</option>)}
          </select>
        </div>
        <div className="ui-field">
          <label htmlFor="tRelease">관련 발매</label>
          <select id="tRelease" value={releaseId} onChange={e => setReleaseId(e.target.value)}>
            <option value="">선택 안 함</option>
            {releases.map(r => <option key={r.id} value={r.id}>{r.title}</option>)}
          </select>
        </div>
      </div>
      <div className="ui-field">
        <label htmlFor="tSubject">문의 제목</label>
        <input id="tSubject" maxLength={180} required placeholder="문의 제목" value={subject} onChange={e => setSubject(e.target.value)} />
      </div>
      <div className="ui-field">
        <label htmlFor="tBody">문의 내용</label>
        <textarea id="tBody" maxLength={4000} rows={6} required placeholder="상황과 확인이 필요한 내용을 자세히 입력해 주세요." value={body} onChange={e => setBody(e.target.value)} />
        <p className="ui-field-help aq-counter">{body.length} / 4000</p>
      </div>
      <div className="aq-sticky-foot">
        <button className="ui-btn is-primary studio-submit-wide" type="submit">문의 저장</button>
      </div>
    </form>
  );
}

export function Inquiries() {
  const toast = useToast();
  const confirm = useConfirm();
  const tickets = ticketsStore.use();
  const [params, setParams] = useSearchParams();
  const cancelFor = params.get('new') === 'cancel' ? params.get('release') ?? '' : '';
  const [showForm, setShowForm] = useState(!!cancelFor);
  const [preset] = useState<TicketPreset | null>(() => (cancelFor ? {
    category: CATEGORIES[0], releaseId: cancelFor, subject: '[발매 취소 요청]',
    body: '이 발매 신청을 취소해 주세요.\n\n취소 사유: ',
  } : null));
  useEffect(() => { if (cancelFor) setParams({}, { replace: true }); }, []); // eslint-disable-line react-hooks/exhaustive-deps
  // 알림의 ‘문의 열기’ 링크 (/inquiries?open=<id>)로 오면 그 대화를 바로 연다
  const [openId, setOpenId] = useState<string | null>(() => params.get('open'));
  useEffect(() => {
    const id = params.get('open');
    if (!id) return;
    setOpenId(id);
    const next = new URLSearchParams(params);
    next.delete('open');
    setParams(next, { replace: true });
  }, [params]); // eslint-disable-line react-hooks/exhaustive-deps
  const openTicket = tickets.find(t => t.id === openId) ?? null;

  const removeTicket = async (t: Ticket) => {
    const ok = await confirm({ title: '문의 기록을 삭제할까요?', message: '삭제하면 되돌릴 수 없어요.', confirmLabel: '삭제', danger: true });
    if (!ok) return;
    ticketsStore.set(ts => ts.filter(x => x.id !== t.id));
    setOpenId(null);
    toast('문의 기록을 삭제했어요.');
  };

  const ordered = tickets.slice().sort((a, b) => b.created.localeCompare(a.created));

  return (
    <div id="view-inquiries" className="view">
      <div className="ui-page-head">
        <div>
          <p className="eyebrow">계정·지원</p>
          <h1>문의</h1>
          <p>발매 보완 요청과 정산·계약 관련 문의를 확인해 보세요.</p>
        </div>
        <button type="button" className="ui-btn is-primary" onClick={() => setShowForm(true)}>
          <Glyph name="plus" size={14} />새 문의
        </button>
      </div>

      {ordered.length ? (
        <div className="aq-catalog-cards aq-stagger">
          {ordered.map(t => (
            <button key={t.id} type="button" className="ui-row aq-ticket-card" onClick={() => setOpenId(t.id)}>
              <span className="aq-ticket-icon" aria-hidden="true"><Glyph name="mail" size={18} /></span>
              <span className="min-0">
                <span className="ui-row-title">{t.subject}</span>
                <span className="ui-row-meta">{t.category} · {niceDate(t.created)} · {t.releaseTitle || '일반 문의'}</span>
              </span>
              <Badge tone={t.status === '답변 완료' ? 'green' : t.status === '종료' ? 'gray' : 'violet'}>{t.status}</Badge>
            </button>
          ))}
        </div>
      ) : (
        <div className="ui-empty">
          <h2>작성한 문의가 없어요.</h2>
          <p>발매·정산·권리 관련 문의 내용을 작성하고 보관할 수 있어요.</p>
          <button type="button" className="ui-btn is-primary" onClick={() => setShowForm(true)}>문의 작성하기</button>
        </div>
      )}

      {showForm && (
        <Modal title="새 문의 작성" onClose={() => setShowForm(false)} dismissible={false}>
          <TicketForm
            preset={preset}
            onSave={async t => {
              if (!MOCK) {
                try {
                  await portal.createInquiry(t);
                  await refreshTickets();
                  toast('문의를 보냈어요. 답변이 오면 알림으로 알려 드릴게요.');
                } catch (err) {
                  toast(errorMessage(err, '문의를 보내지 못했어요. 잠시 후 다시 시도해 주세요.'));
                }
                return;
              }
              ticketsStore.set(ts => [...ts, { ...t, id: uid('q'), created: todayStr(), status: '답변 대기' }]);
              toast('문의 내용을 저장했어요.');
            }}
          />
        </Modal>
      )}

      {openTicket && (
        <Modal title={openTicket.subject} onClose={() => setOpenId(null)}>
          <p className="small muted">{openTicket.category} · {niceDate(openTicket.created)} · {openTicket.releaseTitle || '일반 문의'}</p>
          {!MOCK ? <TicketThread ticket={openTicket} onClosed={() => setOpenId(null)} /> : (<>
          {/* 체험 모드도 실서버와 같은 대화 모양 — 내 문의는 오른쪽, 검토팀 안내는 왼쪽 말풍선 */}
          <ol className="aq-thread">
            <li><span className="aq-thread-who">나 · {niceDate(openTicket.created)}</span><p>{openTicket.body}</p></li>
            <li className="is-staff">
              <span className="aq-thread-who">AUDENIQ 담당자</span>
              <p>{openTicket.status === '답변 완료'
                ? '답변을 등록했어요. 자세한 내용은 가입한 이메일로 안내해 드렸어요.'
                : '문의를 받았어요. 확인 후 가입한 이메일로 답변을 드릴게요.'}</p>
            </li>
          </ol>
          <div className="row-actions" style={{ marginTop: 20 }}>
            <button type="button" className="ui-btn is-danger" onClick={() => removeTicket(openTicket)}>
              문의 기록 삭제
            </button>
          </div>
          </>)}
        </Modal>
      )}
    </div>
  );
}
