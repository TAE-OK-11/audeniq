import { useState } from 'react';
import { Modal } from '../components/Modal';

interface AudenEvent {
  id: string;
  title: string;
  date: string;
  endDate?: string;
  place: string;
  status: 'ongoing' | 'upcoming' | 'ended';
  summary: string;
  body: string;
}

const STATUS_LABEL: Record<AudenEvent['status'], string> = {
  ongoing: '진행 중',
  upcoming: '예정',
  ended: '종료',
};

// TODO: 백엔드 이벤트 API 연동 시 이 mock 데이터를 교체
const EVENTS: AudenEvent[] = [
  {
    id: 'ev1',
    title: 'AUDENIQ 런칭 기념 프로모션',
    date: '2026-10-01',
    endDate: '2026-10-31',
    place: 'AUDENIQ STUDIO',
    status: 'upcoming',
    summary: '정식 런칭을 기념해 첫 발매 수수료 무료 혜택을 드려요.',
    body: 'AUDENIQ 정식 런칭을 기념해 10월 한 달간 첫 발매의 유통 수수료를 무료로 지원합니다.\n\n대상: 2026년 10월 중 발매 접수된 첫 싱글/앨범\n신청: 별도 신청 없이 자동 적용\n\n자세한 내용은 공지사항을 확인해 주세요.',
  },
  {
    id: 'ev2',
    title: '신규 아티스트 온보딩 워크숍',
    date: '2026-09-20',
    place: '온라인',
    status: 'ongoing',
    summary: '발매 위자드부터 정산까지, 스튜디오 사용법을 알려드려요.',
    body: 'AUDENIQ STUDIO의 주요 기능을 60분 안에 익히는 온라인 워크숍입니다.\n\n일정: 2026-09-20 ~ 2026-09-27 (매일 19:00)\n내용: 발매 접수, 권리 서류 준비, 정산·지급 확인\n참가: 문의 페이지에서 "워크숍 참가"로 남겨주시면 초대 링크를 보내드려요.',
  },
  {
    id: 'ev3',
    title: '썸머 플레이리스트 피칭',
    date: '2026-08-01',
    endDate: '2026-08-31',
    place: 'AUDENIQ STUDIO',
    status: 'ended',
    summary: '여름 시즌 플레이리스트에 곡을 추천해 드렸어요.',
    body: '8월 한 달간 진행된 썸머 플레이리스트 피칭 이벤트가 종료됐어요.\n참여해 주신 모든 아티스트분들께 감사드립니다.\n선정 결과는 개별 이메일로 안내드렸어요.',
  },
];

export function Events() {
  const [selected, setSelected] = useState<AudenEvent | null>(null);

  return (
    <div id="view-events" className="view">
      <div className="view-title">
        <div>
          <p className="eyebrow">EVENTS</p>
          <h1>이벤트</h1>
          <p>진행 중인 이벤트와 지난 소식을 확인해 보세요.</p>
        </div>
      </div>

      <div className="aq-catalog-cards">
        {EVENTS.map(ev => (
          <button key={ev.id} type="button" className="aq-event-card" onClick={() => setSelected(ev)}>
            <span className="aq-event-date" aria-hidden="true">
              <b>{ev.date.slice(5, 7)}</b>
              <span>{ev.date.slice(0, 4)}</span>
            </span>
            <span className="min-0">
              <span className="row-name">{ev.title}</span>
              <span className="row-sub">{ev.summary}</span>
              <span className="row-sub">{ev.date}{ev.endDate ? ` ~ ${ev.endDate}` : ''} · {ev.place}</span>
            </span>
            <span className={`status-chip ${ev.status === 'ongoing' ? 'ready' : ev.status === 'upcoming' ? 'pending' : ''}`}>
              {STATUS_LABEL[ev.status]}
            </span>
          </button>
        ))}
      </div>

      {selected && (
        <Modal title={selected.title} onClose={() => setSelected(null)}>
          <p className="eyebrow">{selected.date}{selected.endDate ? ` ~ ${selected.endDate}` : ''} · {selected.place}</p>
          <span className={`status-chip ${selected.status === 'ongoing' ? 'ready' : selected.status === 'upcoming' ? 'pending' : ''}`}>
            {STATUS_LABEL[selected.status]}
          </span>
          <div className="document-body" style={{ marginTop: 16 }}>
            {selected.body.split('\n').map((line, i) => (
              <p key={i}>{line || '\u00A0'}</p>
            ))}
          </div>
        </Modal>
      )}
    </div>
  );
}
