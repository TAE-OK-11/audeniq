import { Link, useLocation } from 'react-router-dom';
import type { ReactNode } from 'react';

const NAV = [
  { to: '/', label: '대시보드' },
  { to: '/releases', label: '발매' },
  { to: '/upload', label: '업로드' },
];

export function Layout({ children }: { children: ReactNode }) {
  const loc = useLocation();
  return (
    <>
      <header className="site-header">
        <div className="nav-shell">
          <Link to="/" className="brand">AUDENIQ <span>STUDIO</span></Link>
          <nav style={{ display: 'flex', gap: 8 }}>
            {NAV.map(n => (
              <Link
                key={n.to}
                to={n.to}
                className={loc.pathname === n.to ? 'btn btn-secondary' : ''}
                style={loc.pathname === n.to ? {} : { padding: '12px 16px', fontSize: 15, fontWeight: 600, color: '#485366' }}
              >
                {n.label}
              </Link>
            ))}
          </nav>
          <span className="workspace-label">STUDIO</span>
        </div>
      </header>
      <main className="portal-layout">{children}</main>
    </>
  );
}
