import { Link, useLocation } from 'react-router-dom';
import type { ReactNode } from 'react';

const NAV = [
  { to: '/', label: '대시보드' },
  { to: '/releases', label: '발매' },
  { to: '/upload', label: '새 발매' },
];

export function Layout({ children }: { children: ReactNode }) {
  const loc = useLocation();
  return (
    <>
      <header className="site-header">
        <div className="nav-shell">
          <Link to="/" className="brand" aria-label="AUDENIQ STUDIO 홈">
            <img src="/connected/assets/AUDENIQ_Logo_Light.svg" alt="AUDENIQ" />
          </Link>
          <nav className="portal-nav">
            {NAV.map(n => (
              <Link key={n.to} to={n.to} className={loc.pathname === n.to ? 'active' : ''}>
                {n.label}
              </Link>
            ))}
          </nav>
        </div>
      </header>
      <main className="portal-layout">{children}</main>
    </>
  );
}
