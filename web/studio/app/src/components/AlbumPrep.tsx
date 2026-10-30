// 발매 상세 '정보' 탭 맨 위 — 이 앨범의 발매 준비 현황 (lib/albumPrep.ts).
// 홈 '확인할 작업'과 같은 줄: 할 일은 누르면 그 자리로 가고, 끝난 일은 초록 체크, AUDENIQ 차례는 회색 시계.
import { useState } from 'react';
import type { PrepItem } from '../lib/albumPrep';
import { Glyph, type GlyphName } from './Glyph';
import '../styles/split/ReleaseDetail.css';

const ICON: Record<PrepItem['key'], GlyphName> = {
  cover: 'image', audio: 'music', credits: 'users', date: 'calendar', agreement: 'sign', proofs: 'doc', codes: 'qr',
};

export function AlbumPrep({ items, go }: { items: PrepItem[]; go: (key: PrepItem['key']) => (() => void) | null }) {
  const done = items.filter(i => i.state === 'done').length;
  const todo = items.filter(i => i.state === 'todo').length;
  // 할 일이 있으면 그것만 먼저 — 끝난 일·AUDENIQ 차례는 접어 둔다 (휴대폰에서 일곱 줄이 화면을 다 차지했다)
  const [all, setAll] = useState(false);
  const shown = todo && !all ? items.filter(i => i.state === 'todo') : items;
  return (
    <section className="aq-relinfo-card aq-prep" aria-labelledby="albumPrepTitle">
      <div className="aq-prep-head">
        <div className="min-0">
          <h2 className="subhead" id="albumPrepTitle">발매 준비 항목</h2>
          <p className="aq-prep-sub">{todo ? `내가 할 일 ${todo}개 · 대기 ${items.filter(i => i.state === 'wait').length}개` : done === items.length ? '모두 끝났어요' : '남은 일은 AUDENIQ이 하고 있어요'}</p>
        </div>
        <p className="aq-prep-count" aria-label={`${items.length}개 중 ${done}개 끝남`}><b>{done}</b>/{items.length}</p>
      </div>
      <div className="aq-prep-bar" aria-hidden="true"><i style={{ ['--p' as string]: `${Math.round((done / items.length) * 100)}%` }} /></div>
      <ul className="aq-task-list aq-prep-list">
        {shown.map(i => {
          const action = i.state === 'todo' ? go(i.key) : null;
          const body = (
            <>
              <span className={`ui-icon${i.state === 'done' ? ' is-green' : i.state === 'wait' ? ' is-gray' : ''}`} aria-hidden="true">
                <Glyph name={i.state === 'done' ? 'check' : i.state === 'wait' ? 'clock' : ICON[i.key]} size={18} />
              </span>
              <span className="aq-task-text">
                <strong>{i.label}</strong>
                <small>{i.sub}</small>
              </span>
            </>
          );
          return (
            // '나머지 보기'로 붙는 줄은 높이가 자라며 펼쳐진다 (lib/expand.ts)
            <li key={i.key} className={`is-${i.state}`} data-expand={todo > 0 && i.state !== 'todo' ? '' : undefined}>
              {action ? (
                <button type="button" className="aq-task" onClick={action}>
                  {body}
                  <span className="aq-task-go">하기<i aria-hidden="true"><Glyph name="chevron-right" size={14} /></i></span>
                </button>
              ) : <div className="aq-task is-static">{body}</div>}
            </li>
          );
        })}
      </ul>
      {todo > 0 && shown.length < items.length || all ? (
        <button type="button" className="aq-task-more" aria-expanded={all} onClick={() => setAll(v => !v)}>
          {all ? '할 일만 보기' : `완료·대기 ${items.length - shown.length}개 보기`}
          <Glyph name="chevron-down" size={14} className={all ? 'is-flip' : undefined} />
        </button>
      ) : null}
    </section>
  );
}
