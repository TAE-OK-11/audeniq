// 약관 (/terms, /terms/settlement) — 로그인 없이 볼 수 있고, 회원가입·배급 신청·계좌 등록·계약서에서 연결한다.
import { useMemo } from 'react';
import { printWithStamp } from '../lib/printStamp';
import { Link, useNavigate } from '../lib/router';
import { LEGAL_DOCS, parseTerms, type LegalKey } from '../lib/terms';
import { LegalBody } from '../components/LegalDoc';

export function Terms({ doc = 'service' }: { doc?: LegalKey }) {
  const nav = useNavigate();
  const d = LEGAL_DOCS[doc];
  const chapters = useMemo(() => parseTerms(d.text).blocks.filter(b => b.kind === 'chapter'), [d.text]);
  return (
    <main className="aq-terms-page">
      <header className="aq-sign-top">
        <Link to="/" aria-label="AUDENIQ"><img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" /></Link>
        <span>{doc === 'privacy' ? '개인정보' : '약관'}</span>
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
        {doc === 'privacy' && <p className="ui-callout" role="note">정식 시행 전 검토본이에요. 시행 정보와 실제 개인정보 처리현황을 확정한 뒤 정식 방침을 안내해요.</p>}
        <nav className="aq-terms-toc" aria-label="장 바로가기">
          <label htmlFor="aqTermsJump" className="sr-only">장 바로가기</label>
          <select id="aqTermsJump" key={doc} defaultValue="" onChange={e => { document.getElementById(e.target.value)?.scrollIntoView({ behavior: 'smooth', block: 'start' }); }}>
            <option value="" disabled>장 바로가기</option>
            {chapters.map(c => c.kind === 'chapter' && <option key={c.id} value={c.id}>{c.text}</option>)}
          </select>
          <button type="button" className="ui-btn is-soft" onClick={() => void printWithStamp({ kind: doc === 'privacy' ? 'PRIVACY' : doc === 'service' ? 'TERMS' : 'SETTLEMENT_TERMS', ref: d.version, title: d.title, root: document.querySelector('.aq-terms') })}>인쇄·저장</button>
        </nav>
        <LegalBody doc={doc} />
      </article>
      <div className="aq-terms-back">
        <button type="button" className="ui-btn is-soft is-sm" onClick={() => (window.history.length > 1 ? window.history.back() : nav('/'))}>돌아가기</button>
      </div>
    </main>
  );
}

export function SettlementTerms() {
  return <Terms doc="settlement" />;
}

export function PrivacyPolicy() {
  return <Terms doc="privacy" />;
}

