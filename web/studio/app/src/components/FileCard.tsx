// 서류 첨부 카드 — 신청서의 커버아트 올리기와 같은 카드형. 카드 전체가 눌리는 영역이고, 끌어다 놓기도 된다.
// 고르기 전에는 안내, 고른 뒤에는 파일 이름과 ‘다시 선택’ 안내가 같은 자리에 보여서 아래로 늘어나지 않는다.
import { useState, type ChangeEvent } from 'react';
import { Glyph } from './Glyph';

export function FileCard({ id, accept, fileName, onFile, busy, hint = 'PDF · JPG · PNG', title = '서류를 끌어다 놓거나 눌러서 선택', status }: {
  id: string; accept?: string; fileName?: string; busy?: boolean; hint?: string; title?: string;
  /** 고른 뒤 아래 줄 (예: ‘서버에 등록됨’) — 없으면 ‘다시 선택’ 안내 */
  status?: string;
  onFile: (file: File, input?: HTMLInputElement) => void;
}) {
  const [over, setOver] = useState(false);
  const has = !!fileName;
  const pick = (e: ChangeEvent<HTMLInputElement>) => {
    const f = e.target.files?.[0];
    if (f) onFile(f, e.target);
    e.target.value = '';
  };
  return (
    <label
      className={`aq-dropzone aq-filecard${over ? ' is-over' : ''}${has ? ' has-file' : ''}${busy ? ' is-busy' : ''}`}
      onDragOver={e => { e.preventDefault(); setOver(true); }}
      onDragLeave={() => setOver(false)}
      onDrop={e => { e.preventDefault(); setOver(false); const f = e.dataTransfer.files?.[0]; if (f) onFile(f); }}
    >
      <input type="file" id={id} accept={accept} disabled={busy} onChange={pick} />
      <span className="aq-dropzone-icon" aria-hidden="true">
        {has ? <Glyph name="doc" size={26} /> : <Glyph name="plus" size={26} />}
      </span>
      <span className="aq-dropzone-text">
        <strong>{busy ? '올리는 중…' : fileName || title}</strong>
        <small>{has ? status || '다른 파일로 바꾸려면 다시 눌러 주세요.' : hint}</small>
      </span>
    </label>
  );
}
