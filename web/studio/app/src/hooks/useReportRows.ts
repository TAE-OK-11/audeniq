// 리포트 행 — 체험 모드는 예시 데이터, 실서버는 플랫폼 리포트(매칭된 행)를 월·플랫폼·발매별로 받는다.
import { MOCK } from '../lib/mode';
import { REPORT_ROWS, type ReportRow } from '../data/reports';
import { fetchReports, type ReportRange } from '../api/portal';
import { dspLabel } from '../lib/catalog';
import { useAsync } from './useAsync';

const inRange = (period: string, r: ReportRange) =>
  (!r.from || period >= r.from) && (!r.to || period <= r.to);

/** pending — 처음 불러오는 중이라 보여 줄 행이 아직 없다(화면은 같은 자리·크기의 자리 표시를 그린다).
 *  기간을 바꿔 다시 불러올 때는 이전 결과를 그대로 보여 주므로 pending이 아니다 */
export function useReportRows(range: ReportRange = {}): { rows: ReportRow[]; loading: boolean; pending: boolean; error: string } {
  const { data, loading, error } = useAsync(async (): Promise<ReportRow[]> => {
    if (MOCK) return REPORT_ROWS.filter(x => inRange(x.period.slice(0, 7), range));
    const r = await fetchReports(range);
    // 화면은 원화로 표시한다: 원화가 아닌 리포트 금액은 원화로 확정되기 전이라 더하지 않는다(재생 수는 반영).
    return r.rows.map(x => ({
      period: x.month, platform: dspLabel(x.dsp), release: x.release || '미확인 발매', track: '전체', releaseId: x.release_id ?? undefined,
      plays: Number(x.streams) || 0, revenue: (x.currency ?? 'KRW') === 'KRW' ? Math.round(Number(x.revenue) || 0) : 0,
    }));
  }, [range.from, range.to]);
  const pending = !MOCK && data === undefined && !error;
  return { rows: data ?? (MOCK ? REPORT_ROWS.filter(x => inRange(x.period.slice(0, 7), range)) : []), loading, pending, error };
}
