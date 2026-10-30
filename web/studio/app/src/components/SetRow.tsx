// 계정 정보 화면의 설정 한 줄 — 이름 · 오른쪽 값 · ›. 계정 정보의 모든 묶음(아티스트·작가·수익 받기·로그인…)이 같이 쓴다
import { Glyph } from './Glyph';

/** 설정 목록의 한 줄 — 이름 · 오른쪽 값 · › */
export function SetRow({ id, label, value, empty, onClick, danger }: {
  id?: string; label: string; value?: string; empty?: string; onClick: () => void; danger?: boolean;
}) {
  return (
    <button type="button" id={id} className={`aq-set-row${danger ? ' is-danger' : ''}`} onClick={onClick}>
      <span className="aq-set-label">{label}</span>
      {value ? <span className="aq-set-value">{value}</span> : empty ? <span className="aq-set-value is-empty">{empty}</span> : null}
      {!danger && <Glyph name="chevron-right" size={16} className="aq-set-chev" />}
    </button>
  );
}

