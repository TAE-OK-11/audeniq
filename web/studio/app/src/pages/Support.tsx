import { useState } from 'react';

const TICKETS = [
  { id: 't1', title: '데모 트랙 보완 요청 관련 문의', status: '답변 완료', date: '2026-09-23' },
  { id: 't2', title: '정산 금액 확인 요청', status: '답변 대기', date: '2026-09-21' },
  { id: 't3', title: '커버아트 규격 문의', status: '답변 완료', date: '2026-09-18' },
];

const NOTICES = [
  { id: 'n1', title: '여름 EP 심사가 통과됐어요.', date: '2026-09-22', unread: true },
  { id: 'n2', title: '데모 트랙에 보완 요청이 있어요.', date: '2026-09-23', unread: true },
  { id: 'n3', title: '2026년 8월 정산이 확정됐어요.', date: '2026-09-10', unread: false },
];

export function Support() {
  const [tab, setTab] = useState<'tickets' | 'notifications'>('tickets');
  const [filter, setFilter] = useState<'all' | 'unread'>('all');
  const [notices, setNotices] = useState(NOTICES);
  const [msg, setMsg] = useState('');

  const unreadCount = notices.filter(n => n.unread).length;
  const shown = filter === 'all' ? notices : notices.filter(n => n.unread);

  const readAll = () => {
    setNotices(ns => ns.map(n => ({ ...n, unread: false })));
  };

  return (
    <>
      <div className="view-title">
        <div>
          <p className="eyebrow">SUPPORT</p>
          <h1>문의·알림</h1>
          <p>발매 보완 요청과 정산·계약 관련 문의를 확인해 보세요.</p>
        </div>
        <button type="button" className="button" onClick={() => setMsg('새 문의 작성은 테스트 모드에서 생략됩니다.')}>
          새 문의 ↗
        </button>
      </div>

      {msg && <div className="notice" style={{ marginBottom: 16 }}>{msg}</div>}

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
        <div className="data-list">
          {TICKETS.map(t => (
            <div key={t.id} className="track-row">
              <div>
                <span className="row-name">{t.title}</span>
                <span className="row-sub">{t.date}</span>
              </div>
              <div />
              <div className="row-end">
                <span className={`status-chip ${t.status === '답변 완료' ? 'live' : 'review'}`}>{t.status}</span>
              </div>
            </div>
          ))}
        </div>
      ) : (
        <div className="data-list">
          {shown.length ? shown.map(n => (
            <div key={n.id} className="track-row">
              <div>
                <span className="row-name">
                  {n.unread && <span className="status-chip scheduled" style={{ marginRight: 8 }}>NEW</span>}
                  {n.title}
                </span>
                <span className="row-sub">{n.date}</span>
              </div>
              <div />
              <div className="row-end" />
            </div>
          )) : (
            <div className="empty-page">
              <h3>읽지 않은 알림이 없어요.</h3>
              <p>새로운 소식이 오면 여기에 표시돼요.</p>
            </div>
          )}
        </div>
      )}
    </>
  );
}
