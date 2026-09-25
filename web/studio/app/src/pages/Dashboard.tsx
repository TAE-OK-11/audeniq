import { Link, useNavigate } from 'react-router-dom';
import { useEffect, useState } from 'react';
import { mockApi } from '../api/mock';
import type { Release } from '../api/client';

const GRID_ITEMS = [
  {
    to: '/releases', title: '발매·곡 관리', desc: '발매 목록과 곡별 정보를 관리하세요',
    icon: <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="5" width="18" height="15" rx="3"/><path d="M7 5V3m5 2V3m5 2V3M7 10h10M7 14h6"/></svg>,
  },
  {
    to: '/reports', title: '음악 리포트', desc: '플랫폼별 재생과 수익 내역',
    icon: <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 19V5M4 19h17M8 15v-4m5 4V7m5 8V4"/></svg>,
  },
  {
    to: '/settlement', title: '정산·지급', desc: '정산 내역과 지급 요청',
    icon: <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m3 5 4.5 14 4.5-11L16.5 19 21 5M2 11h20M2 15h20"/></svg>,
  },
  {
    to: '/contracts', title: '계약서·권리', desc: '계약 상태와 증빙 서류',
    icon: <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M7 3h8l4 4v13a1 1 0 0 1-1 1H7a2 2 0 0 1-2-2V5a2 2 0 0 1-2-2Z"/><path d="M15 3v5h4M9 12h6M9 16h6"/></svg>,
  },
];

const STATUS_LABEL: Record<string, string> = {
  LIVE: '발매 완료', STAGE1_PASSED: '검토 중', STAGE1_CORRECTION: '보완 필요', DRAFT: '작성 중',
};
const STATUS_CLASS: Record<string, string> = {
  LIVE: 'live', STAGE1_PASSED: 'review', STAGE1_CORRECTION: 'needs', DRAFT: 'draft',
};

function money(n: number): string {
  return new Intl.NumberFormat('ko-KR', { style: 'currency', currency: 'KRW', maximumFractionDigits: 0 }).format(n || 0);
}

// mock 이번 달 리포트 데이터
const MOCK_REPORTS = [
  { period: '2026-09-01', revenue: 45230 },
  { period: '2026-09-08', revenue: 128400 },
  { period: '2026-09-15', revenue: 89300 },
  { period: '2026-09-22', revenue: 156700 },
];

function makeChart(rows: { period: string; revenue: number }[]) {
  const buckets = new Map<string, number>();
  for (const r of rows) {
    const k = r.period.slice(0, 7) || '기타';
    buckets.set(k, (buckets.get(k) || 0) + Number(r.revenue || 0));
  }
  const vals = [...buckets.entries()].sort((a, b) => a[0].localeCompare(b[0])).slice(-8);
  if (!vals.length) {
    return (
      <div className="empty-graph">
        아직 수익 리포트가 없어요.<br />정산이 반영되면 이곳에서 확인할 수 있어요.
      </div>
    );
  }
  const high = Math.max(1, ...vals.map(x => x[1]));
  return (
    <>
      <div className="report-chart" role="img" aria-label="기간별 수익 막대그래프">
        {vals.map(([k, v]) => (
          <div
            key={k}
            className="report-bar"
            style={{ ['--h' as string]: `${Math.max(2, Math.round((v / high) * 100))}%` }}
            title={`${k} · ${money(v)}`}
          />
        ))}
      </div>
      <div className="report-axis">
        <span>{vals[0][0]}</span>
        <span>{vals[vals.length - 1][0]}</span>
      </div>
    </>
  );
}

function ReleaseRow({ r }: { r: Release }) {
  const navigate = useNavigate();
  const st = STATUS_LABEL[r.status] || r.status;
  return (
    <article className="release-row studio-album-row">
      <span className="cover" aria-hidden="true">♪</span>
      <div className="min-0">
        <button type="button" className="row-name" onClick={() => navigate(`/releases/${r.id}`)}>
          {r.title || '제목 없는 발매'}
        </button>
        <span className="row-sub">테스트 레이블 · {r.track_count}곡</span>
        <span className="row-sub">{r.release_date || '발매일 미정'}</span>
      </div>
      <div className="row-end">
        <span className={`status-chip ${STATUS_CLASS[r.status] || ''}`}>{st}</span>
      </div>
    </article>
  );
}

export function Dashboard() {
  const navigate = useNavigate();
  const [releases, setReleases] = useState<Release[]>([]);

  useEffect(() => {
    mockApi.listReleases().then(setReleases).catch(() => {});
  }, []);

  const needs = releases.filter(r => r.status === 'STAGE1_CORRECTION').length;
  const drafts = releases.filter(r => r.status === 'DRAFT').length;
  const unread = 2; // mock 읽지 않은 알림

  const tasks: { icon: string; name: string; sub: string; btn: string; to: string }[] = [];
  if (needs) tasks.push({ icon: '!', name: `보완이 필요한 발매 ${needs}건`, sub: '발매별 제출 정보와 증빙을 확인해 보세요.', btn: '확인', to: '/releases' });
  if (drafts) tasks.push({ icon: '↗', name: `작성 중인 발매 ${drafts}건`, sub: '필수 정보와 권리 항목을 확인해 보세요.', btn: '보기', to: '/releases' });
  if (unread) tasks.push({ icon: '♧', name: `읽지 않은 알림 ${unread}건`, sub: '최근 변경사항을 확인해 보세요.', btn: '확인', to: '/support' });

  const monthSum = MOCK_REPORTS.reduce((n, r) => n + r.revenue, 0);

  return (
    <>
      <div className="view-title">
        <div>
          <p className="eyebrow">AUDENIQ / STUDIO</p>
          <h1>서린님의 작업실</h1>
          <p>발매 현황과 확인할 작업을 한곳에서 살펴보세요.</p>
        </div>
        <Link className="button" to="/upload">새로운 발매</Link>
      </div>

      <div className="portal-hero">
        <div>
          <p className="eyebrow">NEW RELEASE</p>
          <h2>새로운 발매를<br />시작해 보세요.</h2>
          <p>음원과 커버아트, 크레딧을 등록하고 발매를 준비해 보세요.</p>
        </div>
        <Link className="button" to="/upload">발매 등록하기</Link>
      </div>

      <div className="dashboard-grid" aria-label="주요 업무">
        {GRID_ITEMS.map(item => (
          <Link key={item.to} className="service-item" to={item.to}>
            <span className="icon-chip">{item.icon}</span>
            <div>
              <h3>{item.title}</h3>
              <p>{item.desc}</p>
            </div>
          </Link>
        ))}
      </div>

      <div className="dashboard-columns">
        <section className="surface white" aria-labelledby="upcomingTitle">
          <div className="section-top">
            <h2 id="upcomingTitle">내 발매</h2>
            <button type="button" className="link-btn" onClick={() => navigate('/releases')}>전체 보기 ↗</button>
          </div>
          <div id="homeReleases" aria-live="polite">
            {releases.length ? (
              releases.slice(0, 4).map(r => <ReleaseRow key={r.id} r={r} />)
            ) : (
              <div className="empty-note">
                아직 등록한 발매가 없어요.<br />새 발매를 만들면 여기에서 확인할 수 있어요.<br />
                <button className="link-btn" type="button" onClick={() => navigate('/upload')}>발매 등록하기 ↗</button>
              </div>
            )}
          </div>
        </section>
        <section className="surface" aria-labelledby="workTitle">
          <div className="section-top">
            <h2 id="workTitle">확인할 작업</h2>
          </div>
          <div id="homeTasks">
            {tasks.length ? (
              tasks.map((t, i) => (
                <div key={i} className="statement-row aq-action-card">
                  <div className="document-icon">{t.icon}</div>
                  <div>
                    <span className="row-name">{t.name}</span>
                    <span className="row-sub">{t.sub}</span>
                  </div>
                  <button type="button" className="link-btn" onClick={() => navigate(t.to)}>{t.btn}</button>
                </div>
              ))
            ) : (
              <div className="empty-note">지금 확인할 작업이 없어요.</div>
            )}
          </div>
        </section>
      </div>

      <div className="section-top">
        <h2>이번 달 음악 리포트</h2>
        <button className="link-btn" type="button" onClick={() => navigate('/reports')}>리포트 보기 ↗</button>
      </div>
      <section className="surface" aria-label="월별 수익">
        <div id="homeReport">
          <div className="section-top">
            <h2>{money(monthSum)} <span className="small muted">· 이번 달 수익 집계</span></h2>
          </div>
          {makeChart(MOCK_REPORTS)}
          <p className="dashboard-help">플랫폼 보고서를 기준으로 집계한 수익을 확인해 보세요.</p>
        </div>
      </section>
    </>
  );
}
