// 계약·권리 서류 카드 — 라이브 aqDocumentCards의 카드 1개 대응
import { docBadgeTone, docState, docTone, type DocRecord } from '../store/docs';
import { niceDate, stripSampleSuffix } from '../lib/format';
import { CheckIcon } from './Check';
import { Glyph } from './Glyph';
import { Badge } from '../ui';

/** 서류 한 줄 — 줄 전체를 눌러 연다. 상태는 오른쪽 배지, 보완 사유는 제목 아래 한 줄 */
export function DocRow({ c, onOpen }: { c: DocRecord; onOpen: (id: string) => void }) {
  const title = stripSampleSuffix(c.title);
  const prefix = c.releaseTitle ? `${c.releaseTitle} · ` : '';
  const name = prefix && title.startsWith(prefix) ? title.slice(prefix.length) : title;
  const needs = c.reviewStatus === 'needs';
  // 홈 '내 발매'와 같은 흰 줄 — 아이콘 칸 · 제목/한 줄 정보 · 오른쪽 상태 (보완 사유는 한 줄만, 전체는 눌러서)
  return (
    <button type="button" className={`ui-row aq-doc-row ${docTone(c)}`} onClick={() => onOpen(c.id)}>
      <span className="ui-icon" aria-hidden="true">
        {needs ? <Glyph name="alert" size={18} /> : c.kind === 'agreements' ? (c.localSignatureAt ? <CheckIcon size={16} /> : <Glyph name="pencil" size={17} />) : <Glyph name="doc" size={17} />}
      </span>
      <span className="min-0 aq-doc-copy">
        <span className="ui-row-title">{name}</span>
        <span className="aq-doc-meta">{c.releaseTitle || '공통 문서'} · {niceDate(c.created)}{c.fileName ? ` · ${c.fileName}` : ''}</span>
        {needs && c.reviewNote && <span className="aq-doc-note">{c.reviewNote}</span>}
      </span>
      <Badge tone={docBadgeTone(c)} className="aq-doc-state">{docState(c)}</Badge>
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
