// 약관 본문 — /terms 페이지와 '약관 보기' 창이 함께 쓴다.
import { useEffect, useMemo, useRef } from 'react';
import { LEGAL_DOCS, parseTerms, type LegalKey } from '../lib/terms';
import { Modal } from './Modal';

export function LegalBody({ doc, anchor }: { doc: LegalKey; anchor?: string }) {
  const d = LEGAL_DOCS[doc];
  const { meta, blocks } = useMemo(() => parseTerms(d.text), [d.text]);
  const ref = useRef<HTMLDivElement>(null);
  // anchor: 조 제목에 들어 있는 말(예: '개인정보')로 바로 그 조까지 내려 준다
  useEffect(() => {
    if (!anchor) return;
    const t = window.setTimeout(() => {
      const el = [...(ref.current?.querySelectorAll('h2,h3') ?? [])].find(h => h.textContent?.includes(anchor));
      el?.scrollIntoView({ block: 'start' });
    }, 60);
    return () => window.clearTimeout(t);
  }, [anchor]);
  return (
    <div className="aq-terms-body" ref={ref}>
      <p className="aq-terms-meta">{meta.join(' · ')}</p>
      {blocks.map((b, i) => {
        switch (b.kind) {
          case 'chapter': return <h2 key={i} id={b.id}>{b.text}</h2>;
          case 'article': return <h3 key={i} id={b.id}>{b.text}</h3>;
          case 'list': return <ol key={i}>{b.items.map(t => <li key={t}>{t}</li>)}</ol>;
          default: return <p key={i}>{b.text}</p>;
        }
      })}
    </div>
  );
}

/** 지금 하던 일을 떠나지 않고 약관을 읽는 창 */
export function LegalModal({ doc, anchor, onClose }: { doc: LegalKey; anchor?: string; onClose: () => void }) {
  return (
    <Modal title={LEGAL_DOCS[doc].title} onClose={onClose} modalClass="aq-legal-mode">
      <div className="aq-terms aq-legal-sheet">
        <LegalBody doc={doc} anchor={anchor} />
      </div>
    </Modal>
  );
}
