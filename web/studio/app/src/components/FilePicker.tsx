// 파일 선택 — 브라우저 기본 파일 입력 대신 같은 모양의 버튼 + 파일 이름.
// iOS 사파리는 기본 입력에 파일 미리보기 사각형을 그려 넣어 모양이 깨지므로, 실제 입력은 화면에서 숨기고
// label로 연다(키보드 포커스는 그대로 받는다). 파일을 고른 뒤에는 버튼이 ‘파일 변경’으로 바뀐다.
import type { ChangeEvent } from 'react';
import { Glyph } from './Glyph';

export function FilePicker({ id, accept, fileName, onChange, disabled, busy, placeholder = '선택한 파일이 없어요' }: {
  id: string; accept?: string; fileName?: string; disabled?: boolean; busy?: boolean; placeholder?: string;
  onChange: (e: ChangeEvent<HTMLInputElement>) => void;
}) {
  const has = !!fileName;
  return (
    <div className={`aq-file${has ? ' has-file' : ''}${disabled ? ' is-disabled' : ''}`}>
      <input type="file" id={id} className="aq-file-input" accept={accept} disabled={disabled || busy} onChange={onChange} />
      <label htmlFor={id} className="aq-file-btn" aria-disabled={disabled || busy || undefined}>
        {has ? '파일 변경' : '파일 선택'}
      </label>
      <span className={`aq-file-name${has ? '' : ' is-empty'}`} title={fileName || undefined}>
        {has && <Glyph name="doc" size={14} className="aq-file-icon" />}
        <span>{busy ? '올리는 중…' : fileName || placeholder}</span>
      </span>
    </div>
  );
}
