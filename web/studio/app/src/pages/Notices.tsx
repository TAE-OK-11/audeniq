import { useState } from 'react';

interface Notice {
  id: string;
  title: string;
  date: string;
  pinned?: boolean;
  body: string;
}

// TODO: 백엔드 공지사항 API 연동 시 이 mock 데이터를 교체
const NOTICES: Notice[] = [
  {
    id: 'n1',
    title: 'AUDENIQ STUDIO 정식 서비스 안내',
    date: '2026-09-26',
    pinned: true,
    body: 'AUDENIQ STUDIO가 정식 서비스를 시작합니다.\n\n이제 발매 접수부터 정산 확인까지 모든 과정을 스튜디오에서 진행할 수 있어요.\n\n이용 중 궁금한 점은 문의 페이지에서 남겨주세요.',
  },
  {
    id: 'n2',
    title: '정산·지급 페이지 개편 안내',
    date: '2026-09-26',
    body: '정산·지급 페이지가 더 간결하게 바뀌었어요.\n\n요청 전 잔액을 한눈에 확인하고, 정산 내역과 지급 요청 기록은 탭으로 나눠 볼 수 있습니다.\n수령 계좌 정보도 상단에서 바로 확인하고 변경할 수 있어요.',
  },
  {
    id: 'n3',
    title: '9월 시스템 점검 안내 (완료)',
    date: '2026-09-15',
    body: '9월 15일 새벽에 진행된 시스템 점검이 완료됐습니다.\n\n점검 시간: 2026-09-15 02:00 ~ 04:00 (KST)\n영향: 점검 시간 중 발매 접수 일시 중단\n\n이용에 불편을 드려 죄송합니다.',
  },
];

export function Notices() {
  const [openId, setOpenId] = useState<string | null>(NOTICES[0]?.id ?? null);

  const ordered = NOTICES.slice().sort((a, b) => {
    if (!!a.pinned !== !!b.pinned) return a.pinned ? -1 : 1;
    return b.date.localeCompare(a.date);
  });
  const pinned = ordered.filter(n => n.pinned);
  const regular = ordered.filter(n => !n.pinned);

  return (
    <div id="view-notices" className="view">
      <div className="view-title">
        <div>
          <p className="eyebrow">NOTICES</p>
          <h1>공지사항</h1>
          <p>꼭 알아야 할 소식과 업데이트를 전해 드려요.</p>
        </div>
      </div>

      {pinned.length > 0 && (
        <>
          <div className="aq-notice-featured-list">
            {pinned.map(n => (
              <button
                key={n.id} type="button" className="aq-notice-featured"
                onClick={() => setOpenId(openId === n.id ? null : n.id)}
                aria-expanded={openId === n.id}
              >
                <span className="aq-pin-badge">고정</span>
                <span className="aq-notice-featured-title">{n.title}</span>
                <span className="aq-notice-featured-date">{n.date}</span>
                {openId === n.id && (
                  <span className="aq-notice-featured-body">
                    {n.body.split('\n').map((line, i) => (
                      <span key={i}>{line || '\u00A0'}<br /></span>
                    ))}
                  </span>
                )}
              </button>
            ))}
          </div>
          <div className="section-top"><h2>전체 공지</h2></div>
        </>
      )}

      <div className="aq-notice-list">
        {regular.map(n => {
          const open = openId === n.id;
          return (
            <div key={n.id} className="aq-notice-item">
              <button
                type="button" className="aq-notice-head"
                aria-expanded={open}
                onClick={() => setOpenId(open ? null : n.id)}
              >
                <span className="min-0">
                  <span className="row-name">
                    {n.pinned && <em className="aq-pin-badge">고정</em>}
                    {n.title}
                  </span>
                  <span className="row-sub">{n.date}</span>
                </span>
                <span className={`aq-chevron${open ? ' is-open' : ''}`} aria-hidden="true">›</span>
              </button>
              {open && (
                <div className="aq-notice-body">
                  {n.body.split('\n').map((line, i) => (
                    <p key={i}>{line || '\u00A0'}</p>
                  ))}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
