import { HashRouter, Routes, Route, Navigate } from 'react-router-dom';
import { AuthProvider, useAuth } from './api/auth';
import { Layout } from './components/Layout';
import { ToastProvider } from './components/Toast';
import { Dashboard } from './pages/Dashboard';
import { Releases } from './pages/Releases';
import { ReleaseDetail } from './pages/ReleaseDetail';
import { Upload } from './pages/Upload';
import { Login } from './pages/Login';
import { Reports } from './pages/Reports';
import { Settlement } from './pages/Settlement';
import { Contracts } from './pages/Contracts';
import { Rights } from './pages/Rights';
import { Support } from './pages/Support';
import { Profile } from './pages/Profile';
import './styles/design.css';
import './styles/live.css';

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
        <ToastProvider>
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
                  <Route path="/reports" element={<Reports />} />
                  <Route path="/settlement" element={<Settlement />} />
                  <Route path="/contracts" element={<Contracts />} />
                  <Route path="/rights" element={<Rights />} />
                  <Route path="/support" element={<Support />} />
                  <Route path="/profile" element={<Profile />} />
                </Routes>
              </Layout>
            </Protected>
          } />
        </Routes>
        </ToastProvider>
      </AuthProvider>
    </HashRouter>
  );
}
