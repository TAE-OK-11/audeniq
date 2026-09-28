// 리포트 행 — 체험 모드는 예시 데이터, 실서버는 플랫폼 리포트(매칭된 행)를 월·플랫폼·발매별로 받는다.
import { MOCK } from '../lib/mode';
import { REPORT_ROWS, type ReportRow } from '../data/reports';
import { fetchReports } from '../api/portal';
import { useDspLabel } from '../lib/catalog';
import { useAsync } from './useAsync';

export function useReportRows(): { rows: ReportRow[]; loading: boolean; error: string } {
  const dspLabel = useDspLabel();
  const { data, loading, error } = useAsync(async () => {
    if (MOCK) return null;
    return fetchReports();
  }, []);
  const rows: ReportRow[] = MOCK ? REPORT_ROWS : (data?.rows ?? []).map(x => ({
      period: x.month, platform: dspLabel(x.dsp), release: x.release || '미확인 발매', track: '전체',
      plays: Number(x.streams) || 0, revenue: Math.round(Number(x.revenue) || 0),
    }));
  return { rows, loading, error };
}
