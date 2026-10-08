// 약관 (/terms, /terms/settlement) — 로그인 없이 볼 수 있고, 회원가입·배급 신청·계약서·계좌 등록에서 연결한다.
import { useMemo } from 'react';
import { Link, useLocation, useNavigate } from '../lib/router';
import { TERMS_DOCS, parseTerms } from '../lib/terms';

export function Terms() {
  const nav = useNavigate();
  const { pathname } = useLocation();
  const doc = TERMS_DOCS.find(d => d.path !== '/terms' && pathname.startsWith(d.path)) ?? TERMS_DOCS[0];
  const { meta, blocks } = useMemo(() => parseTerms(doc.text), [doc]);
  const chapters = blocks.filter(b => b.kind === 'chapter');
  return (
    <div className="aq-terms-page">
      <header className="aq-sign-top">
        <Link to="/" aria-label="AUDENIQ"><img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" /></Link>
        <span>약관</span>
      </header>
      <article key={doc.key} className="aq-terms" aria-labelledby="aqTermsTitle">
        <nav className="aq-terms-docs" aria-label="약관 종류">
          {TERMS_DOCS.map(d => (
            <Link key={d.key} to={d.path} replace aria-current={d === doc ? 'page' : undefined}>{d.short}</Link>
          ))}
        </nav>
        <p className="eyebrow">{doc.version}</p>
        <h1 id="aqTermsTitle">{doc.title}</h1>
        <ul className="aq-terms-meta">{meta.map(m => <li key={m}>{m}</li>)}</ul>
        <nav className="aq-terms-toc" aria-label="장 바로가기">
          <label htmlFor="aqTermsJump" className="sr-only">장 바로가기</label>
          <select id="aqTermsJump" defaultValue="" onChange={e => { document.getElementById(e.target.value)?.scrollIntoView({ behavior: 'smooth', block: 'start' }); }}>
            <option value="" disabled>장 바로가기</option>
            {chapters.map(c => c.kind === 'chapter' && <option key={c.id} value={c.id}>{c.text}</option>)}
          </select>
          <button type="button" className="button secondary" onClick={() => window.print()}>인쇄·저장</button>
        </nav>
        {blocks.map((b, i) => {
          switch (b.kind) {
            case 'chapter': return <h2 key={i} id={b.id}>{b.text}</h2>;
            case 'article': return <h3 key={i} id={b.id}>{b.text}</h3>;
            case 'list': return <ol key={i}>{b.items.map(t => <li key={t}>{t}</li>)}</ol>;
            default: return <p key={i}>{b.text}</p>;
          }
        })}
      </article>
      <div className="aq-terms-back">
        <button type="button" className="link-btn" onClick={() => (window.history.length > 1 ? window.history.back() : nav('/'))}>돌아가기</button>
      </div>
    </div>
  );
}
