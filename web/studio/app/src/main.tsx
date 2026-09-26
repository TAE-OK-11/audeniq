import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { prefetchInitialRoute } from './routes';
import { MOCK } from './api/client';

const root = document.getElementById('root');
if (!root) throw new Error('#root 요소를 찾을 수 없어요.');

// 새 버전 배포로 옛 청크 프리로드가 실패하면 한 번만 새로고침 (무한 새로고침 방지 플래그 공유)
const RELOAD_FLAG = 'aq.chunk-reload';
window.addEventListener('vite:preloadError', e => {
  if (sessionStorage.getItem(RELOAD_FLAG)) return;
  e.preventDefault();
  sessionStorage.setItem(RELOAD_FLAG, '1');
  window.location.reload();
});
// 정상적으로 10초 이상 동작하면 플래그를 지워 다음 배포 때 다시 자동 복구되게 한다
window.setTimeout(() => sessionStorage.removeItem(RELOAD_FLAG), 10000);

prefetchInitialRoute();

// 체험 모드: 접수된 계약서 검토를 자동 완료 (별도 청크로 분리해 실제 모드 번들에는 포함되지 않음)
if (MOCK) void import('./store/mockReviewer').then(m => m.startMockReviewer());

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
