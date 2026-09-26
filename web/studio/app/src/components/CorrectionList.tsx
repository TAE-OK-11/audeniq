import { useNavigate } from '../lib/router';
import type { Correction } from '../api/types';
import { correctionWhere, fixPath, resolveCorrection } from '../lib/corrections';

/** 보완 요청 목록 — 항목을 누르면 신청서의 해당 입력칸으로 바로 이동 */
export function CorrectionList({ releaseId, corrections, trackIds = [], trackTitles = {} }: {
  releaseId: string;
  corrections: Correction[];
  trackIds?: string[];
  trackTitles?: Record<string, string>;
}) {
  const nav = useNavigate();
  return (
    <ul className="aq-correction-list">
      {corrections.map((c, i) => {
        const r = resolveCorrection(c, trackIds);
        const track = c.trackId ? trackTitles[c.trackId] : '';
        return (
          <li key={`${c.code}-${c.trackId ?? ''}-${i}`}>
            <button type="button" onClick={() => nav(fixPath(releaseId, c))}>
              <span className="aq-correction-where">
                {correctionWhere(r)}{track ? ` · ${track}` : ''}
              </span>
              <span className="aq-correction-msg">{r.message}</span>
              <span className="aq-correction-go" aria-hidden="true">바로 보완 ›</span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}
