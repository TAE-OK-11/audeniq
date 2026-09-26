import { useMemo } from 'react';
import { Link, useParams, useSearchParams } from '../lib/router';
import { MOCK } from '../lib/mode';
import { fetchNotices } from '../api/portal';
import { useAsync } from '../hooks/useAsync';
import { SkeletonRows } from '../components/Skeleton';
import { toKstDate } from '../lib/date';

interface Notice {
  id: string;
  title: string;
  date: string;
  pinned?: boolean;
  body: string;
}

// 체험 모드 공지 — 실서버는 엣지 Worker(D1)의 /api/notices
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
  {
    id: 'n4',
    title: '발매 신청서 양식 변경 안내 (AUD 번호 체계)',
    date: '2026-09-10',
    body: '발매 신청서 번호가 AUD-YYYYMMDD-XXXXXX 형식으로 바뀌었어요.\n\n이전에 접수한 발매도 새 양식의 신청서로 확인할 수 있어요.',
  },
  {
    id: 'n5',
    title: '추석 연휴 고객센터 운영 안내',
    date: '2026-09-01',
    body: '추석 연휴 기간에는 문의 답변이 평소보다 늦어질 수 있어요.\n\n연휴 이후 순서대로 빠르게 답변드릴게요.',
  },
];

const PAGE_SIZE = 10;

/** 2026-09-26 → 2026.09.26 */
const dotted = (d: string) => d.replaceAll('-', '.');

function useNotices() {
  const state = useAsync(async (): Promise<Notice[]> => (MOCK
    ? NOTICES
    : (await fetchNotices()).map(n => ({ id: n.id, title: n.title, body: n.body, pinned: n.pinned, date: toKstDate(n.published_at) }))), []);
  const ordered = useMemo(() => (state.data ?? []).slice().sort((a, b) => b.date.localeCompare(a.date)), [state.data]);
  return { ...state, ordered };
}

function LoadState({ loading, error, reload, empty }: { loading: boolean; error: string | null; reload: () => void; empty: boolean }) {
  if (loading) return <SkeletonRows count={4} />;
  if (error) return (
    <div className="empty-page">
      <h2>공지사항을 불러오지 못했어요.</h2>
      <p>{error}</p>
      <button type="button" className="button secondary" onClick={reload}>다시 불러오기</button>
    </div>
  );
  if (empty) return <div className="empty-page"><h2>등록된 공지가 없어요.</h2><p>새 소식이 생기면 이곳에서 알려 드릴게요.</p></div>;
  return null;
}

function Body({ text }: { text: string }) {
  return <>{text.split('\n').map((line, i) => <p key={i}>{line || '\u00A0'}</p>)}</>;
}

export function Notices() {
  const { data, loading, error, reload, ordered } = useNotices();
  const [params, setParams] = useSearchParams();
  const pinned = ordered.filter(n => n.pinned);
  const pages = Math.max(1, Math.ceil(ordered.length / PAGE_SIZE));
  const page = Math.min(pages, Math.max(1, Number(params.get('page')) || 1));
  const rows = ordered.slice((page - 1) * PAGE_SIZE, page * PAGE_SIZE);
  // 페이지 번호는 현재 페이지 주변 5개까지만
  const first = Math.max(1, Math.min(page - 2, pages - 4));
  const numbers = Array.from({ length: Math.min(5, pages) }, (_, i) => first + i);
  const go = (p: number) => {
    setParams(p > 1 ? { page: String(p) } : {});
    window.scrollTo({ top: 0, behavior: 'smooth' });
  };

  return (
    <div id="view-notices" className="view">
      <div className="view-title">
        <div>
          <p className="eyebrow">NOTICES</p>
          <h1>공지사항</h1>
          <p>꼭 알아야 할 소식과 업데이트를 전해 드려요.</p>
        </div>
      </div>

      <LoadState loading={loading && !data} error={error} reload={reload} empty={!!data && !ordered.length} />

      {pinned.length > 0 && (
        <div className="aq-notice-featured-list">
          {pinned.map(n => (
            <Link key={n.id} to={`/notices/${encodeURIComponent(n.id)}`} className="aq-notice-featured">
              <span className="aq-pin-badge">중요</span>
              <span className="aq-notice-featured-title">{n.title}</span>
              <span className="aq-notice-featured-date">{dotted(n.date)}</span>
            </Link>
          ))}
        </div>
      )}

      {ordered.length > 0 && (
        <section className="aq-nboard" aria-labelledby="aqNoticeAll">
          <div className="aq-nboard-top">
            <h2 id="aqNoticeAll">전체 공지</h2>
            <span className="aq-nboard-count">{ordered.length}건</span>
          </div>
          <ul className="aq-nboard-list">
            {rows.map(n => (
              <li key={n.id}>
                <Link to={`/notices/${encodeURIComponent(n.id)}`} className="aq-nboard-row">
                  <span className="aq-nboard-title">
                    {n.pinned && <em className="aq-nboard-tag">중요</em>}
                    {n.title}
                  </span>
                  <span className="aq-nboard-date">{dotted(n.date)}</span>
                </Link>
              </li>
            ))}
          </ul>
          {pages > 1 && (
            <div className="aq-nboard-pages" role="navigation" aria-label="공지 페이지">
              <button type="button" className="aq-nboard-arrow" disabled={page === 1} onClick={() => go(page - 1)} aria-label="이전 페이지">‹</button>
              {numbers.map(p => (
                <button
                  key={p} type="button" onClick={() => go(p)}
                  className={p === page ? 'is-current' : undefined}
                  aria-current={p === page ? 'page' : undefined}
                >{p}</button>
              ))}
              <button type="button" className="aq-nboard-arrow" disabled={page === pages} onClick={() => go(page + 1)} aria-label="다음 페이지">›</button>
            </div>
          )}
        </section>
      )}
    </div>
  );
}

export function NoticeDetail() {
  const { id } = useParams<{ id: string }>();
  const { data, loading, error, reload, ordered } = useNotices();
  const idx = ordered.findIndex(n => n.id === id);
  const notice = idx >= 0 ? ordered[idx] : null;
  const newer = idx > 0 ? ordered[idx - 1] : null;
  const older = idx >= 0 && idx < ordered.length - 1 ? ordered[idx + 1] : null;

  return (
    <div id="view-notice-detail" className="view">
      <Link to="/notices" className="aq-nboard-back">‹ 공지사항</Link>
      <LoadState loading={loading && !data} error={error} reload={reload} empty={false} />
      {data && !notice && (
        <div className="empty-page">
          <h2>공지를 찾을 수 없어요.</h2>
          <p>삭제됐거나 주소가 바뀌었을 수 있어요.</p>
          <Link to="/notices" className="button secondary">목록으로</Link>
        </div>
      )}
      {notice && (
        <article className="aq-nboard-article">
          <header>
            {notice.pinned && <em className="aq-nboard-tag">중요</em>}
            <h1>{notice.title}</h1>
            <time dateTime={notice.date}>{dotted(notice.date)}</time>
          </header>
          <div className="aq-nboard-content"><Body text={notice.body} /></div>
          <div className="aq-nboard-sibling" role="navigation" aria-label="다른 공지">
            {newer && (
              <Link to={`/notices/${encodeURIComponent(newer.id)}`}><span>다음 글</span><strong>{newer.title}</strong></Link>
            )}
            {older && (
              <Link to={`/notices/${encodeURIComponent(older.id)}`}><span>이전 글</span><strong>{older.title}</strong></Link>
            )}
          </div>
          <div className="aq-nboard-foot">
            <Link to="/notices" className="button secondary">목록으로</Link>
          </div>
        </article>
      )}
    </div>
  );
}
