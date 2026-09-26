import { Suspense, lazy } from 'react';
import { HashRouter, Routes, Route, Navigate } from 'react-router-dom';
import { AuthProvider, useAuth } from './api/auth';
import { Layout } from './components/Layout';
import { ToastProvider } from './components/Toast';
import './styles/design.css';
import './styles/live.css';

// 라우트별 코드 스플리팅 — 첫 진입(로그인) 시 전체 번들을 받지 않도록 분리
const Login = lazy(() => import('./pages/Login').then(m => ({ default: m.Login })));
const Signup = lazy(() => import('./pages/Signup').then(m => ({ default: m.Signup })));
const FindAccount = lazy(() => import('./pages/FindAccount').then(m => ({ default: m.FindAccount })));
const Dashboard = lazy(() => import('./pages/Dashboard').then(m => ({ default: m.Dashboard })));
const Releases = lazy(() => import('./pages/Releases').then(m => ({ default: m.Releases })));
const ReleaseDetail = lazy(() => import('./pages/ReleaseDetail').then(m => ({ default: m.ReleaseDetail })));
const Upload = lazy(() => import('./pages/Upload').then(m => ({ default: m.Upload })));
const Reports = lazy(() => import('./pages/Reports').then(m => ({ default: m.Reports })));
const Settlement = lazy(() => import('./pages/Settlement').then(m => ({ default: m.Settlement })));
const Contracts = lazy(() => import('./pages/Contracts').then(m => ({ default: m.Contracts })));
const Rights = lazy(() => import('./pages/Rights').then(m => ({ default: m.Rights })));
const Inquiries = lazy(() => import('./pages/Inquiries').then(m => ({ default: m.Inquiries })));
const Notifications = lazy(() => import('./pages/Notifications').then(m => ({ default: m.Notifications })));
const Events = lazy(() => import('./pages/Events').then(m => ({ default: m.Events })));
const Notices = lazy(() => import('./pages/Notices').then(m => ({ default: m.Notices })));
const Profile = lazy(() => import('./pages/Profile').then(m => ({ default: m.Profile })));

function Protected({ children }: { children: React.ReactNode }) {
  const { user, loading } = useAuth();
  if (loading) return <p style={{ padding: 40, color: 'var(--muted)' }}>불러오는 중...</p>;
  if (!user) return <Navigate to="/login" replace />;
  return <>{children}</>;
}

function PageFallback() {
  return <p style={{ padding: 40, color: 'var(--portal-muted)' }}>불러오는 중...</p>;
}

export function App() {
  return (
    <HashRouter>
      <AuthProvider>
        <ToastProvider>
        <Suspense fallback={<PageFallback />}>
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
                  <Route path="*" element={<Navigate to="/" replace />} />
                </Routes>
              </Layout>
            </Protected>
          } />
        </Routes>
        </Suspense>
        </ToastProvider>
      </AuthProvider>
    </HashRouter>
  );
}
