import { BrowserRouter, Routes, Route } from 'react-router-dom';
import { Layout } from './components/Layout';
import { Dashboard } from './pages/Dashboard';
import { Releases } from './pages/Releases';
import { ReleaseDetail } from './pages/ReleaseDetail';
import { Upload } from './pages/Upload';
import './styles/design.css';

export function App() {
  return (
    <BrowserRouter basename="/connected">
      <Layout>
        <Routes>
          <Route path="/" element={<Dashboard />} />
          <Route path="/releases" element={<Releases />} />
          <Route path="/releases/:id" element={<ReleaseDetail />} />
          <Route path="/upload" element={<Upload />} />
        </Routes>
      </Layout>
    </BrowserRouter>
  );
}
