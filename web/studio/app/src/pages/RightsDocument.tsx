import { useEffect, useState } from 'react';
import { useNavigate, useParams } from '../lib/router';
import { useDocs, setDocs, docState } from '../store/docs';
import { fetchDocuments } from '../api/portal';
import { MOCK } from '../lib/mode';
import { errorMessage } from '../api/errors';
import { localStamp } from '../lib/format';
import { sha256Hex } from '../lib/application';
import { rightsHashInput } from '../lib/rightsDocument';

export function RightsDocument() {
  const { id } = useParams();
  const nav = useNavigate();
  const doc = useDocs().find(d => d.id === id && d.electronic);
  const [loading, setLoading] = useState(!MOCK);
  const [error, setError] = useState('');
  const [integrity, setIntegrity] = useState<'checking' | 'ok' | 'changed'>('checking');
  useEffect(() => {
    if (MOCK) return;
    let cancelled = false;
    fetchDocuments().then(list => { if (!cancelled) setDocs(list); })
      .catch(e => { if (!cancelled) setError(errorMessage(e)); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [id]);
  useEffect(() => {
    if (!doc?.electronic) return;
    let cancelled = false;
    setIntegrity('checking');
    sha256Hex(rightsHashInput(doc.title, doc.content, { ...doc.electronic, consent: true, signature: doc.localSignatureData }))
      .then(hash => { if (!cancelled) setIntegrity(hash === doc.electronic?.content_hash ? 'ok' : 'changed'); });
    return () => { cancelled = true; };
  }, [doc]);
  if (loading) return <p role="status">문서를 불러오는 중이에요.</p>;
  if (error || !doc?.electronic) return <div className="empty-page"><h1>문서를 확인할 수 없어요.</h1><p>{error || '이 계정에서 볼 수 있는 전자 문서가 없어요.'}</p><button className="button secondary" onClick={() => nav('/rights')}>권리 서류로 돌아가기</button></div>;
  const e = doc.electronic;
  return <div className="aq-doc-view view-enter">
    <div className="aq-doc-toolbar"><button className="link-btn" onClick={() => nav('/rights')}>권리·보완 서류</button><button className="button" onClick={() => window.print()}>인쇄 · PDF 저장</button></div>
    <article className="aq-paper" aria-label="권리자 서명 전자 문서">
      <header className="aq-paper-head"><div className="aq-paper-brand"><img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" /><dl className="aq-paper-meta"><div><dt>문서 번호</dt><dd>AUD-RIGHTS-{e.document_no}</dd></div><div><dt>서식</dt><dd>{e.form}</dd></div><div><dt>서명 일시</dt><dd>{localStamp(doc.localSignatureAt)}</dd></div></dl></div><h1>{doc.title}</h1></header>
      <div className="aq-rights-paper-body">{doc.content}</div>
      <section className="aq-paper-signoff"><p>위 내용을 확인하고 기재한 범위에서 이용을 허락합니다.</p><div className="aq-paper-signer"><span>{e.signer_role}</span><strong>{doc.signerName}</strong><span className="aq-paper-sig"><img src={doc.localSignatureData} alt={`${doc.signerName} 서명`} /></span></div><p>{localStamp(doc.localSignatureAt)}</p></section>
      <footer className="aq-paper-foot"><span>{docState(doc)}</span><span className={`aq-paper-integrity is-${integrity}`}>{integrity === 'ok' ? '문서 확인 일치' : integrity === 'changed' ? '문서 내용 불일치' : '문서 확인 중'}</span><code>{e.content_hash}</code><p>작성 내용·권리자·서명자·서명 이미지로 문서 확인 코드를 계산해 보관합니다. 권리자 서명과 AUDENIQ 검토 상태는 별도로 관리합니다.</p>{doc.reviewNote && <p>검토 메모: {doc.reviewNote}</p>}</footer>
    </article>
  </div>;
}
