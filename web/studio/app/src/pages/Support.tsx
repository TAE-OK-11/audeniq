import { useState } from 'react';
import { Modal } from '../components/Modal';
import { useToast } from '../components/Toast';

interface Ticket {
  id: string; category: string; releaseId: string; releaseTitle: string;
  subject: string; body: string; created: string; status: string;
}

interface Notice {
  id: string; kind: string; title: string; detail: string; time: string; read: boolean;
}

const INITIAL_TICKETS: Ticket[] = [
  { id: 't1', category: '발매·심사', releaseId: '', releaseTitle: '데모 트랙', subject: '데모 트랙 보완 요청 관련 문의', body: '보완 요청 항목 중 커버아트 규격이 궁금해요.', created: '2026-09-23', status: '답변 완료' },
  { id: 't2', category: '정산·지급', releaseId: '', releaseTitle: '', subject: '정산 금액 확인 요청', body: '2026년 8월 정산 금액의 상세 내역을 확인하고 싶어요.', created: '2026-09-21', status: '답변 대기' },
];

const INITIAL_NOTICES: Notice[] = [
  { id: 'n1', kind: '발매', title: '여름 EP 심사가 통과됐어요.', detail: '여름 EP가 모든 플랫폼 심사를 통과했어요. 발매일에 맞춰 순차적으로 송출될 예정이에요.', time: '2026-09-22', read: false },
  { id: 'n2', kind: '발매', title: '데모 트랙에 보완 요청이 있어요.', detail: '데모 트랙의 커버아트 해상도가 규격에 맞지 않아요. 3000×3000 이상으로 다시 등록해 주세요.', time: '2026-09-23', read: false },
  { id: 'n3', kind: '지급', title: '2026년 8월 정산이 확정됐어요.', detail: '2026년 8월 정산 ₩24,406이 확정됐어요. 지급 요청은 정산·지급 화면에서 할 수 있어요.', time: '2026-09-10', read: true },
];

const CATEGORIES = ['발매·심사', '수정·테이크다운', '정산·지급', '계약·권리', '계정·기타'];

function noticeSymbol(kind: string): string {
  return kind === '지급' ? '₩' : kind === '발매' ? '♫' : '•';
}

export function Support() {
  const toast = useToast();
  const [tab, setTab] = useState<'tickets' | 'notifications'>('tickets');
  const [filter, setFilter] = useState<'all' | 'unread'>('all');
  const [tickets, setTickets] = useState<Ticket[]>(INITIAL_TICKETS);
  const [notices, setNotices] = useState<Notice[]>(INITIAL_NOTICES);
  const [showForm, setShowForm] = useState(false);
  const [openTicket, setOpenTicket] = useState<Ticket | null>(null);
  const [openNotice, setOpenNotice] = useState<Notice | null>(null);

  const unreadCount = notices.filter(n => !n.read).length;
  const shown = (filter === 'all' ? notices : notices.filter(n => !n.read))
    .slice().sort((a, b) => b.time.localeCompare(a.time));

  const readAll = () => {
    setNotices(ns => ns.map(n => ({ ...n, read: true })));
    toast('모든 알림을 읽음으로 표시했어요.');
  };

  const openNoticeDetail = (n: Notice) => {
    setNotices(ns => ns.map(x => x.id === n.id ? { ...x, read: true } : x));
    setOpenNotice(n);
  };

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
    <>
      <div className="view-title">
        <div>
          <p className="eyebrow">SUPPORT</p>
          <h1>문의·알림</h1>
          <p>발매 보완 요청과 정산·계약 관련 문의를 확인해 보세요.</p>
        </div>
        <button type="button" className="button" onClick={() => setShowForm(true)}>
          새 문의 ↗
        </button>
      </div>

      <div className="tabs" role="tablist" aria-label="문의·알림">
        <button
          type="button" className="tab" role="tab"
          aria-selected={tab === 'tickets'}
          onClick={() => setTab('tickets')}
        >문의 내역</button>
        <button
          type="button" className="tab" role="tab"
          aria-selected={tab === 'notifications'}
          onClick={() => setTab('notifications')}
        >알림</button>
      </div>

      {tab === 'notifications' && (
        <div className="studio-alert-tools">
          <button
            type="button"
            className={`studio-alert-filter${filter === 'all' ? ' active' : ''}`}
            aria-pressed={filter === 'all'}
            onClick={() => setFilter('all')}
          >전체 알림</button>
          <button
            type="button"
            className={`studio-alert-filter${filter === 'unread' ? ' active' : ''}`}
            aria-pressed={filter === 'unread'}
            onClick={() => setFilter('unread')}
          >읽지 않음 {unreadCount > 0 && <span>{unreadCount}</span>}</button>
          <button type="button" className="link-btn" onClick={readAll}>모두 읽음</button>
        </div>
      )}

      {tab === 'tickets' ? (
        tickets.length ? (
          <div className="data-list">
            {tickets.slice().reverse().map(t => (
              <div key={t.id} className="ticket-row">
                <span className="document-icon" aria-hidden="true">✉</span>
                <div className="min-0">
                  <span className="row-name">{t.subject}</span>
                  <span className="row-sub">{t.category} · {t.created}{t.releaseTitle ? ` · ${t.releaseTitle}` : ''}</span>
                </div>
                <button className="link-btn" type="button" onClick={() => setOpenTicket(t)}>열기 ↗</button>
              </div>
            ))}
          </div>
        ) : (
          <div className="empty-page">
            <h3>작성한 문의가 없어요.</h3>
            <p>발매·정산·권리 관련 문의 내용을 작성하고 보관할 수 있어요.</p>
          </div>
        )
      ) : (
        shown.length ? (
          <div className="studio-notice-list">
            {shown.map(n => (
              <button
                key={n.id} type="button"
                className={`studio-notice ${n.read ? 'is-read' : ''}`}
                onClick={() => openNoticeDetail(n)}
              >
                <span className="studio-notice-symbol">{noticeSymbol(n.kind)}</span>
                <span className="studio-notice-copy">
                  <span className="studio-notice-meta">{n.kind} · {n.time}</span>
                  <strong>{n.title}</strong>
                  <span>{n.detail}</span>
                </span>
                {n.read ? null : <i className="studio-unread" aria-label="읽지 않음" />}
              </button>
            ))}
          </div>
        ) : (
          <div className="empty-page">
            <h3>{filter === 'unread' ? '읽지 않은 알림이 없어요.' : '새 알림이 없어요.'}</h3>
            <p>발매·정산·서류 변경 사항을 여기에서 확인할 수 있어요.</p>
          </div>
        )
      )}

      {showForm && (
        <Modal title="새 문의 작성" onClose={() => setShowForm(false)}>
          <p className="small muted">문의 내용을 작성하고 필요한 발매를 연결해 주세요. 운영팀으로 전송되지 않아요.</p>
          <form onSubmit={saveTicket} style={{ marginTop: 16 }}>
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
            <button className="button" type="submit">문의 저장</button>
          </form>
        </Modal>
      )}

      {openTicket && (
        <Modal title={openTicket.subject} onClose={() => setOpenTicket(null)}>
          <p className="small muted">{openTicket.category} · {openTicket.created} · {openTicket.releaseTitle || '일반 문의'}</p>
          <div className="document-body" style={{ marginTop: 16, whiteSpace: 'pre-wrap' }}>{openTicket.body}</div>
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

      {openNotice && (
        <Modal title={openNotice.title} onClose={() => setOpenNotice(null)}>
          <div className="studio-notice-detail">
            <span className="eyebrow">{openNotice.kind} · {openNotice.time}</span>
            <p>{openNotice.detail}</p>
            <div className="doc-connection">
              관련 발매 또는 정산 내역은 각 관리 화면에서 확인할 수 있어요.
            </div>
          </div>
        </Modal>
      )}
    </>
  );
}
