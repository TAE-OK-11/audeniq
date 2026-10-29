import { claimChunkReload, clearChunkReload } from './lib/chunkRecovery';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { prefetchInitialRoute } from './routes';

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

// 키보드로 옮겨 다닐 때만 입력칸에 포커스 테두리 (html.aq-kbd) — 누르거나 탭해 입력할 때 모습은 그대로
const root_ = document.documentElement;
addEventListener('keydown', e => { if (e.key === 'Tab') root_.classList.add('aq-kbd'); }, { capture: true, passive: true });
addEventListener('pointerdown', () => root_.classList.remove('aq-kbd'), { capture: true, passive: true });

// iOS Safari는 터치 리스너가 없으면 :active를 적용하지 않는다 — 누르는 순간의 피드백을 켠다
document.addEventListener('touchstart', () => {}, { passive: true });
// 누름 피드백·펼침·탭 줄·키보드 보조는 첫 화면을 그린 뒤 한가할 때 받는다 (lib/interactions).
// 그 전에 먼저 누르거나 키를 치면 그때 바로 받는다
const loadInteractions = () => { void import('./lib/interactions').then(m => m.initInteractions()); };
const early = () => { loadInteractions(); removeEventListener('pointerdown', early, true); removeEventListener('keydown', early, true); };
addEventListener('pointerdown', early, { capture: true, passive: true });
addEventListener('keydown', early, { capture: true, passive: true });
if ('requestIdleCallback' in window) window.requestIdleCallback(early, { timeout: 2000 });
else setTimeout(early, 1200);

prefetchInitialRoute();

// 체험 모드: 접수된 계약서 검토를 자동 완료 (별도 청크로 분리해 실제 모드 번들에는 포함되지 않음)
// MOCK(lib/mode)과 같은 식을 그대로 쓴다 — 가져온 상수로는 실서버 빌드에도 청크가 만들어졌다
if (import.meta.env.MODE !== 'edge' && import.meta.env.VITE_MOCK !== 'false') void import('./store/mockReviewer').then(m => m.startMockReviewer());

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
