import { useNavigate } from '../lib/router';
import type { Correction } from '../api/types';
import { Glyph } from './Glyph';
import { WIZ_STEP_NAMES, fixPath, resolveCorrection } from '../lib/corrections';

/** 담당자의 전체 의견 — 고칠 항목이 아니라 참고할 말이라 버튼 없이 검토팀 말풍선으로 보여 준다 (신청서 보완 패널과 같은 모양) */
export function ReviewNote({ text }: { text: string }) {
  return (
    <figure className="aq-fix-note">
      <span className="aq-fix-note-avatar" aria-hidden="true">A</span>
      <div className="aq-fix-note-bubble">
        <figcaption>AUDENIQ 검토팀</figcaption>
        <blockquote><p>{text}</p></blockquote>
      </div>
    </figure>
  );
}

/** 보완 요청 할 일 줄 — 단계 이름표 · 무엇을 · ‘고치러 가기’(신청서의 해당 입력칸으로 바로 이동) */
export function CorrectionList({ releaseId, corrections, trackIds = [], trackTitles = {} }: {
  releaseId: string;
  corrections: Correction[];
  trackIds?: string[];
  trackTitles?: Record<string, string>;
}) {
  const nav = useNavigate();
  return (
    <ul className="aq-fix-list">
      {corrections.map((c, i) => {
        const r = resolveCorrection(c, trackIds);
        const track = c.trackId ? trackTitles[c.trackId] : '';
        return (
          <li key={`${c.code}-${c.trackId ?? ''}-${i}`}>
            <span className="aq-fix-step">{r.step + 1}단계 · {WIZ_STEP_NAMES[r.step]}</span>
            <p><strong>{r.label}{track ? ` · ${track}` : ''}</strong>{r.message}</p>
            <button type="button" className="ui-btn is-sm is-soft" onClick={() => nav(fixPath(releaseId, c))}>
              고치러 가기<Glyph name="chevron-right" size={13} />
            </button>
          </li>
        );
      })}
    </ul>
  );
}
