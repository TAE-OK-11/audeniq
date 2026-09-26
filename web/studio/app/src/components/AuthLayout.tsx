import type { ReactNode } from 'react';
import { Link } from 'react-router-dom';

export function AuthLayout({ title, sub, children }: { title: string; sub: string; children: ReactNode }) {
  return (
    <div className="auth-page">
      <div className="auth-split">
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
