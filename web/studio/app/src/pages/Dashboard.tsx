import { Link, useNavigate } from 'react-router';
import { api } from '../api/client';
import { STATUS_LABEL, money, num } from '../lib/format';
import { useUnreadCount } from '../store/support';
import { useProfile } from '../store/profile';
import { useDocs } from '../store/docs';
import { useAsync } from '../hooks/useAsync';
import { SkeletonRows } from '../components/Skeleton';
import { CountUp } from '../components/CountUp';
import { ReleaseCover } from '../components/ReleaseCover';
import { latestSummary, periodLabel } from '../data/reports';
import type { CSSVarStyle } from '../hooks/useAnimations';
import { useGrowOnView } from '../hooks/useAnimations';

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
    icon: <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M7 3h8l4 4v13a1 1 0 0 1-1 1H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2Z"/><path d="M15 3v5h4M9 12h6M9 16h6"/></svg>,
  },
];

function greeting(): string {
  const h = new Date().getHours();
  if (h < 6) return '늦은 밤에도 반가워요.';
  if (h < 12) return '좋은 아침이에요.';
  if (h < 18) return '좋은 오후예요.';
  return '오늘도 수고했어요.';
}

function MiniTrend({ data }: { data: { period: string; revenue: number }[] }) {
  const ref = useGrowOnView<HTMLDivElement>('.aq-mini-bar');
  const high = Math.max(1, ...data.map(d => d.revenue));
  return (
    <div className="aq-mini-trend" ref={ref} role="img" aria-label="최근 월별 수익 추이">
      {data.map((d, i) => (
        <span key={d.period} className="aq-mini-col">
          <span
            className={`aq-mini-bar${i === data.length - 1 ? ' is-current' : ''}`}
            style={{ '--h': `${Math.max(6, Math.round((d.revenue / high) * 100))}%`, '--d': `${i * 80}ms` } as CSSVarStyle}
            title={`${periodLabel(d.period)} · ${money(d.revenue)}`}
          />
          <small>{Number(d.period.slice(5))}월</small>
        </span>
      ))}
    </div>
  );
}

export function Dashboard() {
  const navigate = useNavigate();
  const profile = useProfile();
  const unread = useUnreadCount();
  const docs = useDocs();
  const { data: releases = [], loading, error, reload } = useAsync(() => api.listReleases(), []);
  const report = latestSummary();

  const needs = releases.filter(r => r.status === 'needs').length;
  const drafts = releases.filter(r => r.status === 'draft').length;
  const toSign = docs.filter(d => d.kind === 'agreements' && d.reviewStatus === 'approved' && !d.localSignatureAt).length;
  const docFix = docs.filter(d => d.kind === 'rights' && d.reviewStatus === 'needs').length;

  const profileName = profile.name.trim();
  const tasks: { icon: string; name: string; sub: string; btn: string; to: string; warn?: boolean }[] = [];
  if (needs) tasks.push({ icon: '!', name: `보완이 필요한 발매 ${needs}건`, sub: '발매별 제출 정보와 증빙을 확인해 보세요.', btn: '확인', to: '/releases', warn: true });
  if (docFix) tasks.push({ icon: '!', name: `보완 요청된 서류 ${docFix}건`, sub: '요청 사유를 확인하고 새 원본을 제출해 주세요.', btn: '제출', to: '/rights', warn: true });
  if (drafts) tasks.push({ icon: '↗', name: `작성 중인 발매 ${drafts}건`, sub: '필수 정보와 권리 항목을 확인해 보세요.', btn: '보기', to: '/releases' });
  if (toSign) tasks.push({ icon: '✎', name: `서명할 계약서 ${toSign}건`, sub: '검토가 끝난 계약서에 서명해 주세요.', btn: '서명', to: '/contracts' });
  if (!profileName) tasks.push({ icon: '◉', name: '아티스트 정보 등록', sub: '활동명과 연락처를 입력해 주세요.', btn: '등록', to: '/profile' });
  if (unread) tasks.push({ icon: '♧', name: `읽지 않은 알림 ${unread}건`, sub: '최근 변경사항을 확인해 보세요.', btn: '확인', to: '/notifications' });

  return (
    <div id="view-home" className="view">
      <div className="view-title">
        <div>
          <p className="eyebrow">AUDENIQ / STUDIO</p>
          <h1>{profileName ? `${profileName}님의 작업실` : '내 작업실'}</h1>
          <p>{greeting()} 발매 현황과 확인할 작업을 한곳에서 살펴보세요.</p>
        </div>
        <Link className="button" to="/upload">새로운 발매</Link>
      </div>

      <div className="portal-hero aq-hero-animated">
        <div className="aq-hero-orbs" aria-hidden="true"><i /><i /><i /></div>
        <div>
          <p className="eyebrow">NEW RELEASE</p>
          <h2>새로운 발매를<br />시작해 보세요.</h2>
          <p>음원과 커버아트, 크레딧을 등록하고 발매를 준비해 보세요.</p>
        </div>
        <Link className="button aq-hero-cta" to="/upload">발매 등록하기 <span aria-hidden="true">→</span></Link>
      </div>

      <div className="dashboard-grid aq-stagger" aria-label="주요 업무">
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
        <section className="surface" aria-labelledby="upcomingTitle">
          <div className="section-top">
            <h2 id="upcomingTitle">내 발매</h2>
            <button type="button" className="link-btn" onClick={() => navigate('/releases')}>전체 보기 ↗</button>
          </div>
          <div id="homeReleases" aria-live="polite">
            {loading ? (
              <SkeletonRows count={3} />
            ) : error ? (
              <div className="empty-note">
                {error}<br />
                <button className="link-btn" type="button" onClick={reload}>다시 불러오기</button>
              </div>
            ) : releases.length ? (
              <div className="aq-stagger">
                {releases.slice(0, 4).map(r => (
                  <Link key={r.id} to={`/releases/${r.id}`} className="release-row studio-album-row aq-row-link">
                    <ReleaseCover id={r.id} src={r.coverData} />
                    <div className="min-0">
                      <span className="row-name">{r.title || '제목 없는 발매'}</span>
                      <span className="row-sub">{r.artist || '아티스트 미입력'} · {r.track_count}곡 · {r.release_date || '발매일 미정'}</span>
                    </div>
                    <div className="row-end">
                      <span className={`status-chip ${r.status || 'draft'}`}>{STATUS_LABEL[r.status] || r.status}</span>
                    </div>
                  </Link>
                ))}
              </div>
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
            {tasks.length > 0 && <span className="aq-count-pill">{tasks.length}</span>}
          </div>
          <div id="homeTasks">
            {loading ? (
              <SkeletonRows count={2} />
            ) : tasks.length ? (
              <div className="aq-stagger">
                {tasks.map(t => (
                  <div key={t.name} className={`statement-row aq-action-card${t.warn ? ' is-warning' : ''}`}>
                    <div className="document-icon">{t.icon}</div>
                    <div>
                      <span className="row-name">{t.name}</span>
                      <span className="row-sub">{t.sub}</span>
                    </div>
                    <button type="button" className="link-btn" onClick={() => navigate(t.to)}>{t.btn}</button>
                  </div>
                ))}
              </div>
            ) : (
              <div className="empty-note aq-all-done">
                <span aria-hidden="true">✓</span>
                지금 확인할 작업이 없어요.
              </div>
            )}
          </div>
        </section>
      </div>

      <div className="section-top">
        <h2>{periodLabel(report.period)} 음악 리포트</h2>
        <button className="link-btn" type="button" onClick={() => navigate('/reports')}>리포트 보기 ↗</button>
      </div>
      <section className="surface aq-report-card" aria-label="이번 달 리포트 요약">
        <div className="aq-report-total">
          <small>이번 달 수익</small>
          <strong><CountUp value={report.revenue} format={money} /></strong>
          {report.change != null && (
            <span className={`aq-report-delta${report.change < 0 ? ' is-down' : ''}`}>
              전월 대비 {report.change >= 0 ? '+' : ''}{report.change.toFixed(1)}%
            </span>
          )}
        </div>
        <MiniTrend data={report.trend} />
        <div className="aq-report-grid">
          <div><small>총 재생</small><strong><CountUp value={report.plays} format={n => `${num(n)}회`} /></strong></div>
          <div><small>Top 트랙</small><strong>{report.topTrack || '—'}</strong></div>
          <div><small>Top 플랫폼</small><strong>{report.topPlatform || '—'}</strong></div>
        </div>
      </section>
    </div>
  );
}
