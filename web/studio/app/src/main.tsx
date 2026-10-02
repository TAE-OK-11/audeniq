import { claimChunkReload, clearChunkReload } from './lib/chunkRecovery';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { prefetchInitialRoute } from './routes';
import { MOCK } from './api/client';

const root = document.getElementById('root');
if (!root) throw new Error('#root 요소를 찾을 수 없어요.');

// 새 버전 배포로 옛 청크 프리로드가 실패하면 한 번만 새로고침 (무한 새로고침 방지 플래그 공유)
window.addEventListener('vite:preloadError', e => {
  if (!claimChunkReload()) return;
  e.preventDefault();
  window.location.reload();
});
// 정상적으로 10초 이상 동작하면 플래그를 지워 다음 배포 때 다시 자동 복구되게 한다
window.setTimeout(clearChunkReload, 10000);

// iOS Safari는 16px보다 작은 입력칸을 누르면 화면을 확대하고 되돌리지 않는다.
// iOS에서만 maximum-scale=1을 붙여 자동 확대를 막는다 (iOS는 이 값과 무관하게 두 손가락 확대를 허용하고,
// 안드로이드는 확대를 막아 버리므로 붙이지 않는다). iPadOS는 Mac으로 보고하므로 터치 지원으로 구분.
const isIOS = /iP(hone|od|ad)/.test(navigator.userAgent) || (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1);
if (isIOS) {
  const vp = document.querySelector<HTMLMetaElement>('meta[name=viewport]');
  if (vp && !/maximum-scale/.test(vp.content)) vp.content += ',maximum-scale=1';
}

// iOS Safari는 터치 리스너가 없으면 :active를 적용하지 않는다 — 누르는 순간의 피드백을 켠다
document.addEventListener('touchstart', () => {}, { passive: true });

prefetchInitialRoute();

// 체험 모드: 접수된 계약서 검토를 자동 완료 (별도 청크로 분리해 실제 모드 번들에는 포함되지 않음)
if (MOCK) void import('./store/mockReviewer').then(m => m.startMockReviewer());

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
