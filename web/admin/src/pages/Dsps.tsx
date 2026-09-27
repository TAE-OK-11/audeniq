// DSP 현황 — D-1..D-11 사양(각 플랫폼이 요구하는 조건 전부)과 전송 경로·온보딩, 지금 막힌 발매.
// 콘텐츠 문제로 막힌 패키지가 있거나 연동이 끊기면 카드가 바로 빨간색이 된다.
import { Link } from '../lib/router';
import { useAsync } from '../hooks/useAsync';
import { staffApi } from '../api/staff';
import { reqsFor } from '../dspReqs';
import { Chip, Empty, ErrorBox, PageHead, Skeleton, SubTabs } from '../ui';

export function Dsps() {
  const { data, loading, error, reload } = useAsync(() => staffApi.dsps(), []);
  const blocked = useAsync(() => staffApi.deliveries({ approval: 'PENDING', readiness: 'CONTENT_BLOCKED' }).catch(() => ({ items: [] })), []);
  const items = data?.items ?? [];
  const blockedBy = (code: string) => (blocked.data?.items ?? []).filter(b => b.dsp === code);
  return (
    <div className="view-enter">
      <PageHead
        eyebrow="PLATFORMS" title="플랫폼별 조건"
        sub="플랫폼마다 요구하는 조건과 연결 상태예요. 조건에 맞지 않는 발매가 막혀 있거나 연동에 문제가 있으면 빨간색으로 표시돼요."
        actions={<button type="button" className="adm-btn soft small" onClick={() => { reload(); blocked.reload(); }}>새로고침</button>}
      />
      <SubTabs tabs={[{ to: '/deliveries', label: '배급 현황' }, { to: '/dsps', label: '플랫폼별 조건' }]} />
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <Skeleton rows={3} /> : items.length === 0 ? <Empty title="DSP 정보가 없어요" /> : (
        <div className="adm-dsp-grid">
          {items.map(d => {
            const code = d.code ?? d.dsp;
            const r = d.route;
            const live = !!r?.delivery_enabled && !!r?.adapter_can_send;
            const stuck = blockedBy(code);
            const bad = stuck.length > 0 || (!!r && r.delivery_enabled && !r.adapter_can_send);
            return (
              <div key={code} className={`adm-dsp${bad ? ' is-bad' : ''}`}>
                <div className="adm-dsp-top">
                  <h3>{d.name}</h3>
                  {bad
                    ? <Chip tone="red">{stuck.length ? `막힌 발매 ${stuck.length}건` : 'DSP 점검 필요'}</Chip>
                    : <Chip tone={live ? 'green' : 'gray'}>{live ? '전송 가능' : 'DSP 연동 대기'}</Chip>}
                </div>
                <span className="small muted">{d.region === 'Kr' ? '국내' : '해외'} 플랫폼</span>
                <ul>
                  {reqsFor(d).filter(q => q.key !== 'route').map(q => (
                    <li key={q.key}><span>{q.label}</span><b>{q.spec(d)}</b></li>
                  ))}
                  {r && <li><span>연동</span><b>{live ? '완료' : r.recipient_dpid_registered ? '테스트 중' : '연동 준비 중'}</b></li>}
                </ul>
                {stuck.length > 0 && (
                  <div className="adm-alert is-error" style={{ marginTop: 12 }}>
                    {stuck.slice(0, 3).map(b => (
                      <div key={b.package_id}><Link to={`/reviews/${b.release_id}`}>{b.title}</Link> · {b.blockers.join(', ')}</div>
                    ))}
                    {stuck.length > 3 && <div>외 {stuck.length - 3}건 — <Link to="/deliveries">배급 현황에서 보기</Link></div>}
                  </div>
                )}
                {r && !live && r.onboarding_gaps.length > 0 && (
                  <p className="small muted" style={{ marginTop: 10 }}>연동을 마치려면 {r.onboarding_gaps.length}가지 준비가 더 필요해요.</p>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
