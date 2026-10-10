// 리포트 행 — 체험 모드는 예시 데이터, 실서버는 플랫폼 리포트(매칭된 행)를 월·플랫폼·발매별로 받는다.
import { MOCK } from '../lib/mode';
import { REPORT_ROWS, type ReportRow } from '../data/reports';
import { fetchReports } from '../api/portal';
import { dspLabel } from '../lib/catalog';
import { useAsync } from './useAsync';

export function useReportRows(): { rows: ReportRow[]; loading: boolean; error: string } {
  const { data, loading, error } = useAsync(async (): Promise<ReportRow[]> => {
    if (MOCK) return REPORT_ROWS;
    const r = await fetchReports();
    // 화면은 원화로 표시한다: 원화가 아닌 리포트 금액은 원화로 확정되기 전이라 더하지 않는다(재생 수는 반영).
    return r.rows.map(x => ({
      period: x.month, platform: dspLabel(x.dsp), release: x.release || '미확인 발매', track: '전체',
      plays: Number(x.streams) || 0, revenue: (x.currency ?? 'KRW') === 'KRW' ? Math.round(Number(x.revenue) || 0) : 0,
    }));
  }, []);
  return { rows: data ?? (MOCK ? REPORT_ROWS : []), loading, error };
}
