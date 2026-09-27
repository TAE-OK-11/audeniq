// DSP 현황 — D-1..D-11 레지스트리(사양)와 전송 경로·온보딩 진행 상태 (조회 전용).
import { useAsync } from '../hooks/useAsync';
import { staffApi } from '../api/staff';
import { Chip, Empty, ErrorBox, PageHead, Skeleton } from '../ui';

export function Dsps() {
  const { data, loading, error, reload } = useAsync(() => staffApi.dsps(), []);
  const items = data?.items ?? [];
  return (
    <div className="view-enter">
      <PageHead eyebrow="DSP REGISTRY" title="DSP 현황" sub="플랫폼별 배급 사양과 전송 경로 연결 상태예요. 경로가 없거나 온보딩 중인 DSP는 배급 승인 후에도 연결될 때까지 대기해요." />
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <Skeleton rows={3} /> : items.length === 0 ? <Empty title="DSP 정보가 없어요" /> : (
        <div className="adm-dsp-grid">
          {items.map(d => {
            const r = d.route;
            const live = !!r?.delivery_enabled && !!r?.adapter_can_send;
            return (
              <div key={d.code ?? d.dsp} className="adm-dsp">
                <div className="adm-dsp-top">
                  <h3>{d.name}</h3>
                  <Chip tone={live ? 'green' : r ? 'blue' : 'gray'}>{live ? '전송 가능' : r ? (r.onboarding_stage ?? '연동 중') : '경로 없음'}</Chip>
                </div>
                <span className="adm-code">{d.code ?? d.dsp}</span>
                <ul>
                  <li><span>지역 · 형식</span><b>{d.region === 'Kr' ? '국내' : '글로벌'} · {d.format === 'Ddex' ? 'DDEX' : '파트너 규격'}</b></li>
                  <li><span>리드 타임</span><b>{d.lead_days}일</b></li>
                  <li><span>커버 최소</span><b>{d.artwork_min_px}px</b></li>
                  <li><span>음량 기준</span><b>{d.loudness_target_lufs} LUFS</b></li>
                  {r && <li><span>전송 방식</span><b>{r.transport} · {r.activation_kind}</b></li>}
                  {r && <li><span>수신 DPID</span><b>{r.recipient_dpid_registered ? '등록' : '미등록'}</b></li>}
                </ul>
                {r && r.onboarding_gaps.length > 0 && (
                  <div className="adm-codes" style={{ marginTop: 12 }}>{r.onboarding_gaps.map(g => <Chip key={g} tone="amber">{g}</Chip>)}</div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
