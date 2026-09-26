import { useState } from 'react';
import { useNavigate } from '../lib/router';
import { Modal } from '../components/Modal';
import { useToast } from '../components/Toast';
import { localStamp } from '../lib/format';
import { relativeTime } from '../lib/date';
import { markAllNoticesRead, markNoticeRead, useNotices, useUnreadCount, type Notice } from '../store/support';

function noticeSymbol(kind: string): string {
  return kind === '지급' ? '₩' : kind === '발매' ? '♫' : kind === '서류' ? '▤' : '•';
}

export function Notifications() {
  const toast = useToast();
  const nav = useNavigate();
  const [filter, setFilter] = useState<'all' | 'unread'>('all');
  const notices = useNotices();
  const unread = useUnreadCount();
  const [openNotice, setOpenNotice] = useState<Notice | null>(null);

  const shown = (filter === 'all' ? notices : notices.filter(n => !n.read))
    .slice().sort((a, b) => b.time.localeCompare(a.time));

  const readAll = () => {
    if (!unread) { toast('읽지 않은 알림이 없어요.', 'info'); return; }
    markAllNoticesRead();
    toast('모든 알림을 읽음으로 표시했어요.');
  };

  const openNoticeDetail = (n: Notice) => {
    markNoticeRead(n.id);
    setOpenNotice(n);
  };

  return (
    <div id="view-notifications" className="view">
      <div className="view-title">
        <div>
          <p className="eyebrow">NOTIFICATIONS</p>
          <h1>알림</h1>
          <p>발매·정산·서류 변경 사항을 확인해 보세요.</p>
        </div>
      </div>

      <div className="studio-alert-tools" id="notificationTools">
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
        >읽지 않음 <span id="unreadCount">{unread}</span></button>
        <button type="button" className="link-btn" id="readAllNotices" onClick={readAll} disabled={!unread}>모두 읽음</button>
      </div>

      {shown.length ? (
        <div className="aq-catalog-cards aq-stagger" key={filter}>
          {shown.map(n => (
            <button
              key={n.id} type="button"
              className={`aq-notice-card${n.read ? ' is-read' : ''}`}
              onClick={() => openNoticeDetail(n)}
            >
              <span className="aq-notice-symbol" aria-hidden="true">{noticeSymbol(n.kind)}</span>
              <span className="min-0 aq-notice-copy">
                <span className="aq-notice-meta">{n.kind || '안내'} · {relativeTime(n.time) || n.time}</span>
                <strong>{n.title}</strong>
                <span className="aq-notice-detail">{n.detail}</span>
              </span>
              {n.read ? null : <i className="aq-unread" aria-label="읽지 않음" />}
            </button>
          ))}
        </div>
      ) : (
        <div className="empty-page">
          <h2>{filter === 'unread' ? '읽지 않은 알림이 없어요.' : '새 알림이 없어요.'}</h2>
          <p>발매·정산·서류 변경 사항을 여기에서 확인할 수 있어요.</p>
        </div>
      )}

      {openNotice && (
        <Modal title={openNotice.title} onClose={() => setOpenNotice(null)}>
          <div className="studio-notice-detail">
            <span className="eyebrow">{openNotice.kind || '알림'} · {localStamp(openNotice.time)}</span>
            <p>{openNotice.detail || '상세 내용이 없어요.'}</p>
            {openNotice.link ? (
              <button
                type="button" className="button studio-submit-wide" style={{ marginTop: 18 }}
                onClick={() => { const to = openNotice.link!; setOpenNotice(null); nav(to); }}
              >관련 화면으로 이동 ↗</button>
            ) : (
              <div className="doc-connection">관련 발매 또는 정산 내역은 각 관리 화면에서 확인할 수 있어요.</div>
            )}
          </div>
        </Modal>
      )}
    </div>
  );
}
