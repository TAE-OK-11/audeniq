// 지급 요청 — ADMIN 전용 조회 화면. 실제 지급은 운영 도구에서만 한다 (백엔드도 읽기 전용).
import { useSearchParams } from '../lib/router';
import { useAsync } from '../hooks/useAsync';
import { staffApi } from '../api/staff';
import { PAYOUT_STATUS, ago, pick, shortId, when } from '../labels';
import { Empty, ErrorBox, Filters, PageHead, Skeleton, StatusChip } from '../ui';
import { Glyph } from '../components/Glyph';

const STATUSES = ['REQUESTED', 'ORDERED', 'REJECTED', 'CANCELLED'];
const krw = new Intl.NumberFormat('ko-KR', { style: 'currency', currency: 'KRW', maximumFractionDigits: 0 });
const money = (n: number) => krw.format(Number.isFinite(n) ? n : 0);

export function Payouts() {
  const [params, setParams] = useSearchParams();
  const status = params.get('status') || 'REQUESTED';
  const { data, loading, error, reload } = useAsync(() => staffApi.payouts(status), [status]);
  const items = data?.items ?? [];
  const total = items.reduce((s, p) => s + Number(p.amount || 0), 0);

  return (
    <div className="view-enter">
      <PageHead
        eyebrow="정산"
        title="지급 요청"
        sub="아티스트 정산 지급 요청이에요. 여기서는 확인만 하고, 지급은 운영 도구에서 처리해요."
        actions={<button type="button" className="adm-btn soft small" onClick={reload}>새로고침</button>}
      />
      <Filters label="지급 상태" value={status} onChange={v => setParams({ status: v }, { replace: true })} options={STATUSES.map(s => ({ value: s, label: PAYOUT_STATUS[s][0] }))} />
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <Skeleton rows={3} /> : items.length === 0 ? <Empty icon={<Glyph name="won" size={22} />} title="해당 상태의 지급 요청이 없어요" /> : (
        <>
          <div className="adm-alert">{items.length}건 · 합계 <b>{money(total)}</b></div>
          <div className="adm-card white adm-table-wrap">
            <table className="adm-table">
              <thead><tr><th>작업 공간</th><th>금액</th><th>상태</th><th>지급 지시</th><th>요청</th></tr></thead>
              <tbody>
                {items.map(p => (
                  <tr key={p.id}>
                    <td><b>{p.org_name}</b></td>
                    <td>{p.currency === 'KRW' ? money(Number(p.amount)) : `${p.amount} ${p.currency}`}</td>
                    <td><StatusChip value={pick(PAYOUT_STATUS, p.status)} /></td>
                    <td>{p.payout_order_id ? <span className="adm-code">{shortId(p.payout_order_id)}</span> : '—'}</td>
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
