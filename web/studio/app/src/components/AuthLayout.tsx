import type { ReactNode } from 'react';
import { Link } from 'react-router-dom';

export function AuthLayout({ title, sub, children }: { title: string; sub: string; children: ReactNode }) {
  return (
    <div className="auth-page">
      <div className="auth-split">
        <div className="auth-brand">
          <img src="/connected/assets/AUDENIQ_Logo_Light.svg" alt="AUDENIQ STUDIO" className="auth-brand-logo" />
          <h2>음악을 세상에,<br />가장 먼저.</h2>
          <p>AUDENIQ STUDIO에서 발매부터 정산까지<br />한 곳에서 관리하세요.</p>
          <ul>
            <li>간편한 발매 접수</li>
            <li>투명한 정산 확인</li>
            <li>안전한 권리 관리</li>
          </ul>
        </div>
        <div className="auth-card">
          <h1>{title}</h1>
          <p className="auth-sub">{sub}</p>
          {children}
        </div>
      </div>
      <p className="auth-foot">
        <Link to="/notices">공지사항</Link>
        <span aria-hidden="true">·</span>
        <Link to="/events">이벤트</Link>
      </p>
    </div>
  );
}
