// 계약·권리 서류 카드 — 라이브 aqDocumentCards의 카드 1개 대응
import { docBadgeTone, docState, docTone, type DocRecord } from '../store/docs';
import { niceDate, stripSampleSuffix } from '../lib/format';
import { CheckIcon } from './Check';
import { Glyph } from './Glyph';
import { Badge } from '../ui';

/** 서류 한 줄 — 줄 전체를 눌러 연다. 상태는 배지 대신 오른쪽 색 글자, 보완 사유는 제목 아래 */
export function DocRow({ c, onOpen }: { c: DocRecord; onOpen: (id: string) => void }) {
  const title = stripSampleSuffix(c.title);
  const prefix = c.releaseTitle ? `${c.releaseTitle} · ` : '';
  const name = prefix && title.startsWith(prefix) ? title.slice(prefix.length) : title;
  const needs = c.reviewStatus === 'needs';
  return (
    <button type="button" className={`aq-doc-row ${docTone(c)}`} onClick={() => onOpen(c.id)}>
      <span className="aq-doc-ico" aria-hidden="true">
        {needs ? <Glyph name="alert" size={18} /> : c.kind === 'agreements' ? (c.localSignatureAt ? <CheckIcon size={16} /> : <Glyph name="pencil" size={17} />) : <Glyph name="doc" size={17} />}
      </span>
      <span className="aq-doc-copy">
        <strong>{name}</strong>
        <small>{c.releaseTitle || '공통 문서'} · {niceDate(c.created)}{c.fileName ? ` · ${c.fileName}` : ''}</small>
        {needs && c.reviewNote && <em>{c.reviewNote}</em>}
      </span>
      <Badge tone={docBadgeTone(c)} className="aq-doc-state">{docState(c)}</Badge>
      <Glyph name="chevron-right" size={16} className="aq-doc-chev" />
    </button>
  );
}

/** 빈 목록 플레이스홀더 — 라이브 emptyPage */
export function DocEmpty({ title, desc }: { title: string; desc: string }) {
  return (
    <div className="ui-empty">
      <h2>{title}</h2>
      <p>{desc}</p>
    </div>
  );
}
