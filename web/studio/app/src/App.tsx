import { HashRouter, Routes, Route, Navigate } from 'react-router-dom';
import { AuthProvider, useAuth } from './api/auth';
import { Layout } from './components/Layout';
import { Dashboard } from './pages/Dashboard';
import { Releases } from './pages/Releases';
import { ReleaseDetail } from './pages/ReleaseDetail';
import { Upload } from './pages/Upload';
import { Login } from './pages/Login';
import './styles/design.css';

function Protected({ children }: { children: React.ReactNode }) {
  const { user, loading } = useAuth();
  if (loading) return <p style={{ padding: 40, color: 'var(--muted)' }}>불러오는 중...</p>;
  if (!user) return <Navigate to="/login" replace />;
  return <>{children}</>;
}

export function App() {
  return (
    <HashRouter>
      <AuthProvider>
        <Routes>
          <Route path="/login" element={<Login />} />
          <Route path="/*" element={
            <Protected>
              <Layout>
                <Routes>
                  <Route path="/" element={<Dashboard />} />
                  <Route path="/releases" element={<Releases />} />
                  <Route path="/releases/:id" element={<ReleaseDetail />} />
                  <Route path="/upload" element={<Upload />} />
                </Routes>
              </Layout>
            </Protected>
          } />
        </Routes>
      </AuthProvider>
    </HashRouter>
  );
}
