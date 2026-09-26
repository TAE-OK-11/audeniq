import { HashRouter, Routes, Route, Navigate } from 'react-router-dom';
import { AuthProvider, useAuth } from './api/auth';
import { Layout } from './components/Layout';
import { ToastProvider } from './components/Toast';
import { Dashboard } from './pages/Dashboard';
import { Releases } from './pages/Releases';
import { ReleaseDetail } from './pages/ReleaseDetail';
import { Upload } from './pages/Upload';
import { Login } from './pages/Login';
import { Signup } from './pages/Signup';
import { FindAccount } from './pages/FindAccount';
import { Reports } from './pages/Reports';
import { Settlement } from './pages/Settlement';
import { Contracts } from './pages/Contracts';
import { Rights } from './pages/Rights';
import { Inquiries } from './pages/Inquiries';
import { Notifications } from './pages/Notifications';
import { Events } from './pages/Events';
import { Notices } from './pages/Notices';
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
          <Route path="/signup" element={<Signup />} />
          <Route path="/find-account" element={<FindAccount />} />
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
                  <Route path="/inquiries" element={<Inquiries />} />
                  <Route path="/notifications" element={<Notifications />} />
                  <Route path="/events" element={<Events />} />
                  <Route path="/notices" element={<Notices />} />
                  <Route path="/support" element={<Navigate to="/inquiries" replace />} />
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
