// 약관 (/terms, /terms/settlement) — 로그인 없이 볼 수 있고, 회원가입·배급 신청·계좌 등록·계약서에서 연결한다.
import { useMemo } from 'react';
import { Link, useNavigate } from '../lib/router';
import { LEGAL_DOCS, parseTerms, type LegalKey } from '../lib/terms';
import { LegalBody } from '../components/LegalDoc';

export function Terms({ doc = 'service' }: { doc?: LegalKey }) {
  const nav = useNavigate();
  const d = LEGAL_DOCS[doc];
  const chapters = useMemo(() => parseTerms(d.text).blocks.filter(b => b.kind === 'chapter'), [d.text]);
  return (
    <div className="aq-terms-page">
      <header className="aq-sign-top">
        <Link to="/" aria-label="AUDENIQ"><img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" /></Link>
        <span>약관</span>
      </header>
      <article className="aq-terms" aria-labelledby="aqTermsTitle">
        <nav className="aq-terms-tabs" aria-label="약관 종류">
          {(Object.keys(LEGAL_DOCS) as LegalKey[]).map(k => (
            <Link key={k} to={LEGAL_DOCS[k].path} className={k === doc ? 'is-on' : undefined} aria-current={k === doc ? 'page' : undefined}>
              {LEGAL_DOCS[k].short}
            </Link>
          ))}
        </nav>
        <p className="eyebrow">{d.version}</p>
        <h1 id="aqTermsTitle">{d.title}</h1>
        <nav className="aq-terms-toc" aria-label="장 바로가기">
          <label htmlFor="aqTermsJump" className="sr-only">장 바로가기</label>
          <select id="aqTermsJump" key={doc} defaultValue="" onChange={e => { document.getElementById(e.target.value)?.scrollIntoView({ behavior: 'smooth', block: 'start' }); }}>
            <option value="" disabled>장 바로가기</option>
            {chapters.map(c => c.kind === 'chapter' && <option key={c.id} value={c.id}>{c.text}</option>)}
          </select>
          <button type="button" className="ui-btn is-soft" onClick={() => window.print()}>인쇄·저장</button>
        </nav>
        <LegalBody doc={doc} />
      </article>
      <div className="aq-terms-back">
        <button type="button" className="ui-btn is-soft is-sm" onClick={() => (window.history.length > 1 ? window.history.back() : nav('/'))}>돌아가기</button>
      </div>
    </div>
  );
}

export function SettlementTerms() {
  return <Terms doc="settlement" />;
}
