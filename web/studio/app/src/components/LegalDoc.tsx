// 약관 본문 — /terms 페이지와 '약관 보기' 창이 함께 쓴다.
import { useEffect, useMemo, useRef, useState } from 'react';
import { LEGAL_DOCS, legalTextNow, loadLegalText, parseTerms, type LegalKey } from '../lib/terms';
import { Modal } from './Modal';
import { ReadFailure } from './ReadFailure';

/** 약관 원문 — 문서마다 따로 받는다. 받기 전에는 undefined, 실패하면 null */
export function useLegalText(doc: LegalKey): { text: string | undefined | null; retry: () => void } {
  const [state, setState] = useState<{ doc: LegalKey; text: string | undefined | null }>(() => ({ doc, text: legalTextNow(doc) }));
  const [tick, setTick] = useState(0);
  useEffect(() => {
    let alive = true;
    const now = legalTextNow(doc);
    setState({ doc, text: now });
    if (now !== undefined) return;
    loadLegalText(doc).then(t => { if (alive) setState({ doc, text: t }); }, () => { if (alive) setState({ doc, text: null }); });
    return () => { alive = false; };
  }, [doc, tick]);
  return { text: state.doc === doc ? state.text : legalTextNow(doc), retry: () => setTick(n => n + 1) };
}

export function LegalBody({ doc, anchor }: { doc: LegalKey; anchor?: string }) {
  const { text, retry } = useLegalText(doc);
  const parsed = useMemo(() => (text ? parseTerms(text) : null), [text]);
  const ref = useRef<HTMLDivElement>(null);
  // anchor: 조 제목에 들어 있는 말(예: '개인정보')로 바로 그 조까지 내려 준다
  useEffect(() => {
    if (!anchor || !parsed) return;
    const t = window.setTimeout(() => {
      const el = [...(ref.current?.querySelectorAll('h2,h3') ?? [])].find(h => h.textContent?.includes(anchor));
      el?.scrollIntoView({ block: 'start' });
    }, 60);
    return () => window.clearTimeout(t);
  }, [anchor, parsed]);
  if (text === null) return <ReadFailure label="약관" retry={async () => retry()} />;
  if (!parsed) {
    // 받는 동안 — 제목·문단 자리
    return (
      <div className="aq-terms-body" aria-busy="true" aria-label="약관을 불러오는 중">
        <span className="aq-skel aq-skel-line" style={{ width: '58%' }} />
        <span className="aq-skel aq-skel-title" />
        {[92, 86, 95, 70, 88, 64].map((w, i) => <span key={i} className="aq-skel aq-skel-line" style={{ width: `${w}%`, marginTop: 12 }} />)}
      </div>
    );
  }
  const { meta, blocks } = parsed;
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
