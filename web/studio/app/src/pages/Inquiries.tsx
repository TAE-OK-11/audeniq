import { useState } from 'react';
import { Modal, useModalClose } from '../components/Modal';
import { useToast } from '../components/Toast';
import { useConfirm } from '../components/Confirm';
import { niceDate } from '../lib/format';
import { todayStr } from '../lib/date';
import { uid } from '../lib/store';
import { api } from '../api/client';
import { useAsync } from '../hooks/useAsync';
import { ticketsStore, type Ticket } from '../store/tickets';

const CATEGORIES = ['발매·심사', '수정·테이크다운', '정산·지급', '계약·권리', '계정·기타'];

function TicketForm({ onSave }: { onSave: (t: Omit<Ticket, 'id' | 'created' | 'status'>) => void }) {
  const close = useModalClose();
  const toast = useToast();
  const { data: releases = [] } = useAsync(() => api.listReleases(), []);
  const [category, setCategory] = useState(CATEGORIES[0]);
  const [releaseId, setReleaseId] = useState('');
  const [subject, setSubject] = useState('');
  const [body, setBody] = useState('');

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
        <div className="field">
          <label htmlFor="tCategory">문의 유형</label>
          <select id="tCategory" value={category} onChange={e => setCategory(e.target.value)}>
            {CATEGORIES.map(c => <option key={c}>{c}</option>)}
          </select>
        </div>
        <div className="field">
          <label htmlFor="tRelease">관련 발매</label>
          <select id="tRelease" value={releaseId} onChange={e => setReleaseId(e.target.value)}>
            <option value="">선택 안 함</option>
            {releases.map(r => <option key={r.id} value={r.id}>{r.title}</option>)}
          </select>
        </div>
      </div>
      <div className="field">
        <label htmlFor="tSubject">문의 제목</label>
        <input id="tSubject" maxLength={180} required placeholder="문의 제목" value={subject} onChange={e => setSubject(e.target.value)} />
      </div>
      <div className="field">
        <label htmlFor="tBody">문의 내용</label>
        <textarea id="tBody" maxLength={4000} rows={6} required placeholder="상황과 확인이 필요한 내용을 자세히 입력해 주세요." value={body} onChange={e => setBody(e.target.value)} />
        <p className="help aq-counter">{body.length} / 4000</p>
      </div>
      <button className="button studio-submit-wide" type="submit">문의 저장</button>
    </form>
  );
}

export function Inquiries() {
  const toast = useToast();
  const confirm = useConfirm();
  const tickets = ticketsStore.use();
  const [showForm, setShowForm] = useState(false);
  const [openId, setOpenId] = useState<string | null>(null);
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
      <div className="view-title">
        <div>
          <p className="eyebrow">INQUIRIES</p>
          <h1>문의</h1>
          <p>발매 보완 요청과 정산·계약 관련 문의를 확인해 보세요.</p>
        </div>
        <button type="button" className="button" onClick={() => setShowForm(true)}>
          새 문의 ↗
        </button>
      </div>

      {ordered.length ? (
        <div className="aq-catalog-cards aq-stagger">
          {ordered.map(t => (
            <button key={t.id} type="button" className="aq-ticket-card" onClick={() => setOpenId(t.id)}>
              <span className="aq-ticket-icon" aria-hidden="true">✉</span>
              <span className="min-0">
                <span className="row-name">{t.subject}</span>
                <span className="row-sub">{t.category} · {niceDate(t.created)} · {t.releaseTitle || '일반 문의'}</span>
              </span>
              <span className={`status-chip ${t.status === '답변 완료' ? 'ready' : 'review'}`}>{t.status}</span>
            </button>
          ))}
        </div>
      ) : (
        <div className="empty-page">
          <h2>작성한 문의가 없어요.</h2>
          <p>발매·정산·권리 관련 문의 내용을 작성하고 보관할 수 있어요.</p>
          <button type="button" className="button" onClick={() => setShowForm(true)}>문의 작성하기</button>
        </div>
      )}

      {showForm && (
        <Modal title="새 문의 작성" onClose={() => setShowForm(false)} dismissible={false}>
          <TicketForm
            onSave={t => {
              ticketsStore.set(ts => [...ts, { ...t, id: uid('q'), created: todayStr(), status: '답변 대기' }]);
              toast('문의 내용을 저장했어요.');
            }}
          />
        </Modal>
      )}

      {openTicket && (
        <Modal title={openTicket.subject} onClose={() => setOpenId(null)}>
          <p className="small muted">{openTicket.category} · {niceDate(openTicket.created)} · {openTicket.releaseTitle || '일반 문의'}</p>
          <div className="document-body aq-ticket-body">{openTicket.body}</div>
          <div className="notice" style={{ marginTop: 20 }}>
            {openTicket.status === '답변 완료'
              ? '담당자 답변이 등록된 문의예요. 자세한 답변은 가입한 이메일로 안내돼요.'
              : '담당자가 확인 후 가입한 이메일로 답변을 드릴게요.'}
          </div>
          <div className="row-actions" style={{ marginTop: 20 }}>
            <button type="button" className="button danger" onClick={() => removeTicket(openTicket)}>
              문의 기록 삭제
            </button>
          </div>
        </Modal>
      )}
    </div>
  );
}
