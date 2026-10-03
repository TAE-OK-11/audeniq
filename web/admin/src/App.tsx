import { loginRedirect } from './lib/loginRedirect';
import type { ReactNode } from 'react';
import { BrowserRouter, Navigate, Route, Routes, useLocation } from './lib/router';
import { AuthProvider, useAuth } from './auth';
import { ToastProvider } from './components/Toast';
import { ConfirmProvider } from './components/Confirm';
import { Login } from './Login';
import { AdminApp } from './AdminApp';
// 스튜디오와 같은 디자인 레이어(버튼·입력창·카드·모달·토스트) 위에 관리자 레이아웃(admin.css)
import './styles/design.css';
import './styles/live.css';
import './styles/enhance.css';
import './styles/controls.css';

function BootScreen() {
  return (
    <div className="aq-boot" role="status" aria-label="AUDENIQ ADMIN을 불러오는 중">
      <img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="" />
      <span className="aq-boot-bar" />
    </div>
  );
}

function Protected({ children }: { children: ReactNode }) {
  const { user, loading } = useAuth();
  const loc = useLocation();
  if (loading) return <BootScreen />;
  if (!user) return <Navigate to="/login" replace state={{ from: loc.pathname + loc.search }} />;
  return <>{children}</>;
}

function GuestOnly({ children }: { children: ReactNode }) {
  const { user, loading } = useAuth();
  const loc = useLocation();
  if (loading) return <BootScreen />;
  if (user) {
    const from = loginRedirect(loc.state);
    return <Navigate to={from ?? '/'} replace />;
  }
  return <>{children}</>;
}

export function App() {
  return (
    <BrowserRouter>
      <AuthProvider>
        <ToastProvider>
          <ConfirmProvider>
            <Routes>
              <Route path="/login" element={<GuestOnly><Login /></GuestOnly>} />
              <Route path="/*" element={<Protected><AdminApp /></Protected>} />
            </Routes>
          </ConfirmProvider>
        </ToastProvider>
      </AuthProvider>
    </BrowserRouter>
  );
}
