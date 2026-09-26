import { Suspense, lazy, type ReactNode } from 'react';
import { HashRouter, Routes, Route, Navigate, useLocation } from 'react-router';
import { AuthProvider, useAuth } from './api/auth';
import { Layout } from './components/Layout';
import { ToastProvider } from './components/Toast';
import { ConfirmProvider } from './components/Confirm';
import { ErrorBoundary } from './components/ErrorBoundary';
import { PageSkeleton } from './components/Skeleton';
import './styles/design.css';
import './styles/live.css';
import './styles/enhance.css';

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

function BootScreen() {
  return (
    <div className="aq-boot" role="status" aria-label="AUDENIQ STUDIO를 불러오는 중">
      <img src={`${import.meta.env.BASE_URL}assets/AUDENIQ_Logo_Light.svg`} alt="" />
      <span className="aq-boot-bar" />
    </div>
  );
}

/** 로그인 필요 — 원래 가려던 주소를 기억해 로그인 후 되돌아간다 */
function Protected({ children }: { children: ReactNode }) {
  const { user, loading } = useAuth();
  const loc = useLocation();
  if (loading) return <BootScreen />;
  if (!user) return <Navigate to="/login" replace state={{ from: loc.pathname + loc.search }} />;
  return <>{children}</>;
}

/** 이미 로그인한 사용자가 인증 화면에 오면 홈으로 */
function GuestOnly({ children }: { children: ReactNode }) {
  const { user, loading } = useAuth();
  const loc = useLocation();
  if (loading) return <BootScreen />;
  if (user) {
    const from = (loc.state as { from?: string } | null)?.from;
    return <Navigate to={from && !/^\/(login|signup|find-account)/.test(from) ? from : '/'} replace />;
  }
  return <>{children}</>;
}

/** ?edit= 값이 바뀌면 위자드 상태를 새로 시작 (새 발매 ↔ 수정 전환 시 이전 입력이 남는 문제 방지) */
function UploadRoute() {
  const loc = useLocation();
  return <Upload key={loc.search} />;
}

function PortalRoutes() {
  const loc = useLocation();
  return (
    <Layout>
      <ErrorBoundary resetKey={loc.pathname}>
        <Suspense fallback={<PageSkeleton />}>
          <Routes>
            <Route path="/" element={<Dashboard />} />
            <Route path="/releases" element={<Releases />} />
            <Route path="/releases/:id" element={<ReleaseDetail />} />
            <Route path="/upload" element={<UploadRoute />} />
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
        </Suspense>
      </ErrorBoundary>
    </Layout>
  );
}

export function App() {
  return (
    <HashRouter>
      <AuthProvider>
        <ToastProvider>
          <ConfirmProvider>
            <ErrorBoundary>
              <Suspense fallback={<BootScreen />}>
                <Routes>
                  <Route path="/login" element={<GuestOnly><Login /></GuestOnly>} />
                  <Route path="/signup" element={<GuestOnly><Signup /></GuestOnly>} />
                  <Route path="/find-account" element={<GuestOnly><FindAccount /></GuestOnly>} />
                  <Route path="/*" element={<Protected><PortalRoutes /></Protected>} />
                </Routes>
              </Suspense>
            </ErrorBoundary>
          </ConfirmProvider>
        </ToastProvider>
      </AuthProvider>
    </HashRouter>
  );
}
