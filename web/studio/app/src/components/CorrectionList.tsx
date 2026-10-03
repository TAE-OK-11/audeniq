import { useNavigate } from '../lib/router';
import type { Correction } from '../api/types';
import { Glyph } from './Glyph';
import { correctionWhere, fixPath, resolveCorrection } from '../lib/corrections';

/** 담당자의 전체 의견 — 고칠 항목이 아니라 참고할 말이라 버튼 없이 말풍선 카드로 보여 준다 */
export function ReviewNote({ text }: { text: string }) {
  return (
    <figure className="aq-review-note">
      <figcaption className="aq-review-note-head">
        <span className="aq-review-note-mark" aria-hidden="true"><Glyph name="inquiry" size={15} /></span>
        <span className="aq-review-note-label">담당자 의견</span>
        <span className="aq-review-note-by">AUDENIQ 검토팀</span>
      </figcaption>
      <blockquote><p>{text}</p></blockquote>
    </figure>
  );
}

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
              <span className="aq-correction-no" aria-hidden="true">{i + 1}</span>
              <span className="aq-correction-where">
                {correctionWhere(r)}{track ? ` · ${track}` : ''}
              </span>
              <span className="aq-correction-msg">{r.message}</span>
              <span className="aq-correction-go"><span className="sr-only">바로 보완</span><Glyph name="chevron-right" size={16} /></span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}
