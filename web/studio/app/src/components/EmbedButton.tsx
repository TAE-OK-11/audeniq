// 임베드 버튼 — 블로그·홈페이지에 붙여 넣을 스마트링크 카드 HTML. 미리 보기와 코드 복사.
import { useState } from 'react';
import { Modal } from './Modal';
import { Glyph } from './Glyph';
import { CopyButton } from './CopyButton';
import { embedHtml, type EmbedInput } from '../lib/embed';

export function EmbedButton({ input, className = 'ui-btn is-sm' }: { input: EmbedInput; className?: string }) {
  const [open, setOpen] = useState(false);
  const html = embedHtml(input);
  return (
    <>
      <button type="button" className={className} onClick={() => setOpen(true)}><Glyph name="table" size={14} />임베드</button>
      {open && (
        <Modal title="블로그·홈페이지에 넣기" onClose={() => setOpen(false)} modalClass="aq-set-modal aq-embed-modal">
          <div className="aq-embed">
            <p className="ui-field-label">미리 보기</p>
            {/* 우리가 만든(이스케이프한) 코드라 그대로 그린다 */}
            <div className="aq-embed-preview" dangerouslySetInnerHTML={{ __html: html }} />
            <label className="ui-field-label" htmlFor="embedCode">코드</label>
            <textarea id="embedCode" className="aq-embed-code" readOnly rows={5} value={html} onFocus={e => e.currentTarget.select()} />
            <p className="ui-field-help">블로그 글의 HTML 편집 모드에 붙여 넣으세요. 누르면 스마트링크가 열려요. (HTML을 쓸 수 없는 곳은 주소만 붙여 넣어 주세요.)</p>
            <CopyButton text={html} label="코드 복사" className="ui-btn is-primary is-block" />
          </div>
        </Modal>
      )}
    </>
  );
}
