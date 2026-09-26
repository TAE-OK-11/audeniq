import { Suspense, lazy, type ComponentType, type ReactNode } from 'react';
import { HashRouter, Routes, Route, Navigate, useLocation } from './lib/router';
import { AuthProvider, useAuth } from './api/auth';
import { Layout } from './components/Layout';
import { ToastProvider } from './components/Toast';
import { ConfirmProvider } from './components/Confirm';
import { ErrorBoundary } from './components/ErrorBoundary';
import { PageSkeleton } from './components/Skeleton';
import { pageLoaders } from './routes';
import './styles/design.css';
import './styles/live.css';
import './styles/enhance.css';

// 배포 직후 옛 청크가 사라져 동적 import가 실패하면 한 번만 새로고침해 새 버전을 받는다
const RELOAD_FLAG = 'aq.chunk-reload';
function lazyPage<M>(load: () => Promise<M>, pick: (m: M) => ComponentType) {
  return lazy(async () => {
    try {
      const mod = await load();
      return { default: pick(mod) };
    } catch (e) {
      if (!sessionStorage.getItem(RELOAD_FLAG)) {
        sessionStorage.setItem(RELOAD_FLAG, '1');
        window.location.reload();
        await new Promise(() => {}); // 새로고침될 때까지 대기 (오류 화면 깜빡임 방지)
      }
      throw e;
    }
  });
}

const Login = lazyPage(pageLoaders.Login, m => m.Login);
const Signup = lazyPage(pageLoaders.Signup, m => m.Signup);
const FindAccount = lazyPage(pageLoaders.FindAccount, m => m.FindAccount);
const Dashboard = lazyPage(pageLoaders.Dashboard, m => m.Dashboard);
const Releases = lazyPage(pageLoaders.Releases, m => m.Releases);
const ReleaseDetail = lazyPage(pageLoaders.ReleaseDetail, m => m.ReleaseDetail);
const Application = lazyPage(pageLoaders.Application, m => m.Application);
const Upload = lazyPage(pageLoaders.Upload, m => m.Upload);
const Reports = lazyPage(pageLoaders.Reports, m => m.Reports);
const Settlement = lazyPage(pageLoaders.Settlement, m => m.Settlement);
const Contracts = lazyPage(pageLoaders.Contracts, m => m.Contracts);
const Rights = lazyPage(pageLoaders.Rights, m => m.Rights);
const Inquiries = lazyPage(pageLoaders.Inquiries, m => m.Inquiries);
const Notifications = lazyPage(pageLoaders.Notifications, m => m.Notifications);
const Events = lazyPage(pageLoaders.Events, m => m.Events);
const Notices = lazyPage(pageLoaders.Notices, m => m.Notices);
const Profile = lazyPage(pageLoaders.Profile, m => m.Profile);

function BootScreen() {
  return (
    <div className="aq-boot" role="status" aria-label="AUDENIQ STUDIO를 불러오는 중">
      <img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="" />
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
            <Route path="/releases/:id/application" element={<Application />} />
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
