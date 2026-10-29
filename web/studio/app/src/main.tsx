import { claimChunkReload, clearChunkReload } from './lib/chunkRecovery';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { prefetchInitialRoute } from './routes';
import { initPressFeedback } from './lib/press';
import { initRowNav } from './lib/rowNav';
import { initExpand } from './lib/expand';
import { initTopCover } from './lib/topCover';
import { initEnterNext } from './lib/enterNext';

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
// 짧게 톡 눌러도 눌린 모습이 보이게 (.is-pressed)
initPressFeedback();
// 탭·칩 줄: 고른 칸이 줄 밖이면 보이게, 화살표 키로 옮기기
initRowNav();
// 누르면 나타나는 입력 폼·패널은 높이가 자라며 펼쳐지게
initExpand();
// 키보드가 떠 있는 동안 상태 표시줄 칸으로 뒤 내용이 비치지 않게 (iOS 26)
initTopCover();
// 폼 밖 입력 화면(발매 신청 등): 키보드의 '다음'으로 다음 칸, 마지막 칸은 '완료'로 키보드 내리기
initEnterNext();

prefetchInitialRoute();

// 체험 모드: 접수된 계약서 검토를 자동 완료 (별도 청크로 분리해 실제 모드 번들에는 포함되지 않음)
// MOCK(lib/mode)과 같은 식을 그대로 쓴다 — 가져온 상수로는 실서버 빌드에도 청크가 만들어졌다
if (import.meta.env.MODE !== 'edge' && import.meta.env.VITE_MOCK !== 'false') void import('./store/mockReviewer').then(m => m.startMockReviewer());

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
