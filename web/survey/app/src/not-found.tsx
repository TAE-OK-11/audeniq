// 없는 주소 — Workers Static Assets가 404 상태로 이 페이지를 준다 (not_found_handling: 404-page)
// 공통 디자인 템플릿(web/shared/design)의 헤더·패널·버튼을 그대로 쓴다.
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import '../../../shared/design/tokens.css';
import '../../../shared/design/components.css';
import './not-found.css';

function NotFound() {
  return (
    <>
      <header className="aq-header">
        <div className="aq-header__inner">
          <a href="/" className="aq-brand" aria-label="설문 첫 화면으로"><img src="/assets/AUDENIQ_Logo_Light.svg" alt="AUDENIQ" /></a>
          <span className="aq-header-pill">RESEARCH</span>
        </div>
      </header>
      <main className="nf-main">
        <section className="aq-hero nf-panel">
          <span className="aq-eyebrow aq-eyebrow--lg">404 · NOT FOUND</span>
          <h1>페이지를 찾을 수 없어요.</h1>
          <p>요청한 주소를 다시 확인해 주세요.</p>
          <a className="aq-button aq-button--lg" href="/">설문으로 돌아가기 <span className="aq-button__arrow" aria-hidden="true">→</span></a>
        </section>
      </main>
    </>
  );
}

createRoot(document.getElementById('root')!).render(<StrictMode><NotFound /></StrictMode>);
