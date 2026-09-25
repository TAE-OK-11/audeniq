// 계약·권리 서류 카드 — 라이브 aqDocumentCards의 카드 1개 대응
import { docState, docTone, type DocRecord } from '../store/docs';
import { niceDate, stripSampleSuffix } from '../lib/format';

export function DocCard({ c, onOpen }: { c: DocRecord; onOpen: (id: string) => void }) {
  const btnLabel = c.kind === 'agreements'
    ? (c.localSignatureAt ? '계약서 보기' : '확인하고 서명')
    : (c.reviewStatus === 'needs' ? '보완하기' : '자세히 보기');
  return (
    <article className={`aq-doc-card ${docTone(c)}`}>
      <span className="document-icon" aria-hidden="true">{c.kind === 'agreements' ? '✓' : '▤'}</span>
      <div className="aq-doc-copy">
        <button type="button" className="row-name" onClick={() => onOpen(c.id)}>
          {stripSampleSuffix(c.title)}
        </button>
        <span className="row-sub">{c.releaseTitle || '공통 문서'} · {niceDate(c.created)}</span>
        <div className="aq-doc-state-line">
          <span className="aq-doc-state-pill">{docState(c)}</span>
          {c.fileName && <span>{c.fileName}</span>}
          {c.reviewNote && <span>{c.reviewNote}</span>}
        </div>
      </div>
      <button className="button secondary" type="button" onClick={() => onOpen(c.id)}>
        {btnLabel}
      </button>
    </article>
  );
}

/** 빈 목록 플레이스홀더 — 라이브 emptyPage */
export function DocEmpty({ title, desc }: { title: string; desc: string }) {
  return (
    <div className="empty-page">
      <h2>{title}</h2>
      <p>{desc}</p>
    </div>
  );
}
