// 파일 선택 칸 — OS마다 다르게 그려지던 기본 파일 입력(iOS의 파란 썸네일 등) 대신
// 드롭존과 같은 윤곽선 없는 면 스타일. 실제 <input type=file>은 칸 전체를 덮어 접근성·키보드 동작을 유지한다.
import type { ChangeEvent } from 'react';

const ICONS = {
  audio: 'M9 18V5l12-2v13M9 18a3 3 0 1 1-6 0 3 3 0 0 1 6 0Zm12-2a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z',
  doc: 'M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8l-5-5Zm0 0v5h5M9 13h6M9 17h4',
};

export function FilePick({ id, accept, onChange, fileName, hint, kind = 'doc', done = false, disabled = false }: {
  id: string;
  accept: string;
  onChange: (e: ChangeEvent<HTMLInputElement>) => void;
  /** 선택·등록된 파일 이름 (없으면 안내 문구) */
  fileName?: string;
  /** 파일 이름 아래 작은 안내 */
  hint?: string;
  kind?: keyof typeof ICONS;
  done?: boolean;
  disabled?: boolean;
}) {
  const has = !!fileName;
  return (
    <label className={`aq-filepick${has ? ' has-file' : ''}${done ? ' is-done' : ''}${disabled ? ' is-disabled' : ''}`}>
      <input type="file" id={id} accept={accept} onChange={onChange} disabled={disabled} />
      <span className="aq-filepick-icon" aria-hidden="true">
        {done ? (
          <svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round"><path d="m5 12.5 4.5 4.5L19 7.5" /></svg>
        ) : (
          <svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round"><path d={ICONS[kind]} /></svg>
        )}
      </span>
      <span className="aq-filepick-text">
        <strong>{fileName || '눌러서 파일 선택'}</strong>
        {hint && <small>{hint}</small>}
      </span>
      <span className="aq-filepick-cta" aria-hidden="true">{has ? '변경' : '선택'}</span>
    </label>
  );
}
