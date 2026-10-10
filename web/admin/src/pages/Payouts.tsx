// 지급 요청 — ADMIN 전용 조회 화면. 실제 지급은 운영 도구에서만 한다 (백엔드도 읽기 전용).
import { useSearchParams } from '../lib/router';
import { useAsync } from '../hooks/useAsync';
import { staffApi } from '../api/staff';
import { PAYOUT_STATUS, ago, pick, shortId, when } from '../labels';
import { EmptyState, ErrorBox, ChipGroup, PageHeader, SkeletonList, StatusBadge } from '../staffUi';
import { Glyph } from '../components/Glyph';
import { Money } from '../components/Money';

const STATUSES = ['REQUESTED', 'ORDERED', 'REJECTED', 'CANCELLED'];

export function Payouts() {
  const [params, setParams] = useSearchParams();
  const status = params.get('status') || 'REQUESTED';
  const { data, loading, error, reload } = useAsync(() => staffApi.payouts(status), [status]);
  const items = data?.items ?? [];
  const total = items.reduce((s, p) => s + Number(p.amount || 0), 0);

  return (
    <div className="view-enter">
      <PageHeader
        eyebrow="정산"
        title="지급 요청"
        sub="아티스트 정산 지급 요청이에요. 여기서는 확인만 하고, 지급은 운영 도구에서 처리해요."
        actions={<button type="button" className="ui-btn is-soft is-sm" onClick={reload}>새로고침</button>}
      />
      <ChipGroup label="지급 상태" value={status} onChange={v => setParams({ status: v }, { replace: true })} options={STATUSES.map(s => ({ value: s, label: PAYOUT_STATUS[s][0] }))} />
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <SkeletonList rows={3} /> : items.length === 0 ? <EmptyState icon={<Glyph name="won" size={22} />} title="해당 상태의 지급 요청이 없어요" /> : (
        <>
          <div className="ui-callout">{items.length}건 · 합계 <b><Money value={total} mark /></b></div>
          <div className="ui-card adm-table-wrap">
            <table className="adm-table">
              <thead><tr><th>작업 공간</th><th>금액</th><th>상태</th><th>지급 지시</th><th>요청</th></tr></thead>
              <tbody>
                {items.map(p => (
                  <tr key={p.id}>
                    <td><b>{p.org_name}</b></td>
                    <td><Money value={p.amount} currency={p.currency} /></td>
                    <td><StatusBadge value={pick(PAYOUT_STATUS, p.status)} /></td>
                    <td>{p.payout_order_id ? <span className="ui-tag is-mono">{shortId(p.payout_order_id)}</span> : '—'}</td>
                    <td className="small" title={when(p.created_at)}>{ago(p.created_at)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </>
      )}
    </div>
  );
}
