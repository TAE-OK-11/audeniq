import { HashRouter, Routes, Route, Navigate } from 'react-router-dom';
import { AuthProvider, useAuth } from './api/auth';
import { Layout } from './components/Layout';
import { Dashboard } from './pages/Dashboard';
import { Releases } from './pages/Releases';
import { ReleaseDetail } from './pages/ReleaseDetail';
import { Upload } from './pages/Upload';
import { Login } from './pages/Login';
import { Placeholder } from './pages/Placeholder';
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
                  <Route path="/reports" element={<Placeholder eyebrow="REPORTS" title="음악 리포트" desc="스트리밍과 성과를 확인하세요." />} />
                  <Route path="/settlement" element={<Placeholder eyebrow="SETTLEMENT" title="정산·지급" desc="수익 정산 내역을 확인하세요." />} />
                  <Route path="/contracts" element={<Placeholder eyebrow="CONTRACTS" title="AUDENIQ 계약서" desc="계약서를 확인하고 서명하세요." />} />
                  <Route path="/rights" element={<Placeholder eyebrow="RIGHTS" title="권리·보완 서류" desc="권리 관련 서류를 관리하세요." />} />
                  <Route path="/support" element={<Placeholder eyebrow="SUPPORT" title="문의·알림" desc="도움이 필요하시면 문의하세요." />} />
                  <Route path="/profile" element={<Placeholder eyebrow="PROFILE" title="아티스트·정산 정보" desc="아티스트 정보와 정산 계좌를 관리하세요." />} />
                </Routes>
              </Layout>
            </Protected>
          } />
        </Routes>
      </AuthProvider>
    </HashRouter>
  );
}
