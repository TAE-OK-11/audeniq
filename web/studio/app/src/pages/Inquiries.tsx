import { useState } from 'react';
import { Modal } from '../components/Modal';
import { useToast } from '../components/Toast';
import { niceDate } from '../lib/format';

interface Ticket {
  id: string; category: string; releaseId: string; releaseTitle: string;
  subject: string; body: string; created: string; status: string;
}

const INITIAL_TICKETS: Ticket[] = [
  { id: 't1', category: '발매·심사', releaseId: '', releaseTitle: '데모 트랙', subject: '데모 트랙 보완 요청 관련 문의', body: '보완 요청 항목 중 커버아트 규격이 궁금해요.', created: '2026-09-23', status: '답변 완료' },
  { id: 't2', category: '정산·지급', releaseId: '', releaseTitle: '', subject: '정산 금액 확인 요청', body: '2026년 8월 정산 금액의 상세 내역을 확인하고 싶어요.', created: '2026-09-21', status: '답변 대기' },
];

const CATEGORIES = ['발매·심사', '수정·테이크다운', '정산·지급', '계약·권리', '계정·기타'];

export function Inquiries() {
  const toast = useToast();
  const [tickets, setTickets] = useState<Ticket[]>(INITIAL_TICKETS);
  const [showForm, setShowForm] = useState(false);
  const [openTicket, setOpenTicket] = useState<Ticket | null>(null);

  const saveTicket = (e: React.FormEvent) => {
    e.preventDefault();
    const form = e.target as HTMLFormElement;
    const data = new FormData(form);
    const subject = String(data.get('tSubject') || '').trim();
    const body = String(data.get('tBody') || '').trim();
    if (!subject || !body) { toast('제목과 내용을 입력해 주세요.'); return; }
    setTickets(ts => [...ts, {
      id: 't' + Date.now(),
      category: String(data.get('tCategory') || ''),
      releaseId: String(data.get('tRelease') || ''),
      releaseTitle: '',
      subject, body,
      created: new Date().toISOString().slice(0, 10),
      status: '답변 대기',
    }]);
    setShowForm(false);
    toast('문의 내용을 저장했어요. 운영팀으로 전송되지는 않았어요.');
  };

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

      {tickets.length ? (
        <div className="aq-catalog-cards">
          {tickets.slice().reverse().map(t => (
            <button
              key={t.id} type="button"
              className="aq-ticket-card"
              onClick={() => setOpenTicket(t)}
            >
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
        </div>
      )}

      {showForm && (
        <Modal title="새 문의 작성" onClose={() => setShowForm(false)}>
          <p className="small muted">문의 내용을 작성하고 필요한 발매를 연결해 주세요. 운영팀으로 전송되지 않아요.</p>
          <form id="ticketForm" onSubmit={saveTicket}>
            <div className="field">
              <label htmlFor="tCategory">문의 유형</label>
              <select id="tCategory" name="tCategory">
                {CATEGORIES.map(c => <option key={c}>{c}</option>)}
              </select>
            </div>
            <div className="field">
              <label htmlFor="tRelease">관련 발매</label>
              <select id="tRelease" name="tRelease">
                <option value="">선택 안 함</option>
                <option>첫 번째 싱글</option>
                <option>여름 EP</option>
                <option>데모 트랙</option>
              </select>
            </div>
            <div className="field">
              <label htmlFor="tSubject">문의 제목</label>
              <input id="tSubject" name="tSubject" maxLength={180} required placeholder="문의 제목" />
            </div>
            <div className="field">
              <label htmlFor="tBody">문의 내용</label>
              <textarea id="tBody" name="tBody" maxLength={4000} rows={6} required placeholder="상황과 확인이 필요한 내용을 자세히 입력해 주세요." />
            </div>
            <button className="button studio-submit-wide" type="submit">문의 저장</button>
          </form>
        </Modal>
      )}

      {openTicket && (
        <Modal title={openTicket.subject} onClose={() => setOpenTicket(null)}>
          <p className="small muted">{openTicket.category} · {niceDate(openTicket.created)} · {openTicket.releaseTitle || '일반 문의'}</p>
          <div className="document-body">{openTicket.body}</div>
          <div className="notice" style={{ marginTop: 20 }}>
            현재 문의 내역은 운영팀에 아직 전달되지 않았어요.
          </div>
          <div className="row-actions" style={{ marginTop: 20 }}>
            <button
              type="button" className="button danger"
              onClick={() => {
                setTickets(ts => ts.filter(t => t.id !== openTicket.id));
                setOpenTicket(null);
                toast('문의 기록을 삭제했어요.');
              }}
            >
              문의 기록 삭제
            </button>
          </div>
        </Modal>
      )}
    </div>
  );
}
