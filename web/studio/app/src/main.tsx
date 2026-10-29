import { claimChunkReload, clearChunkReload } from './lib/chunkRecovery';
import { StrictMode, type ComponentType } from 'react';
import { createRoot } from 'react-dom/client';
import { prefetchInitialRoute } from './routes';
import { currentPath } from './lib/router';
import { isPublicPath } from './lib/publicRoute';
import { isChunkError } from './lib/chunkRecovery';

// '지금 새로고침'(SystemStatus)이 캐시를 건너뛰려고 붙인 표시는 열리자마자 주소에서 지운다
if (/[?&]__r=/.test(location.search)) {
  const u = new URL(location.href);
  u.searchParams.delete('__r');
  history.replaceState(history.state, '', u.pathname + u.search + u.hash);
}

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

// 공개 페이지(스마트링크·아티스트)는 공개 앱만, 그 밖은 스튜디오 앱을 받는다 — 팬은 스튜디오 화면 코드·전체 스타일을 받지 않는다.
// 스튜디오 주소면 화면 조각도 함께 받기 시작한다(인증 확인과 나란히)
const publicPage = isPublicPath(currentPath());
if (!publicPage) prefetchInitialRoute();
const loadApp = (): Promise<ComponentType> => (publicPage
  ? import('./public/PublicApp').then(m => m.PublicApp)
  : import('./App').then(m => m.App));

// 체험 모드: 접수된 계약서 검토를 자동 완료 (별도 청크로 분리해 실제 모드 번들에는 포함되지 않음)
// MOCK(lib/mode)과 같은 식을 그대로 쓴다 — 가져온 상수로는 실서버 빌드에도 청크가 만들어졌다
if (import.meta.env.MODE !== 'edge' && import.meta.env.VITE_MOCK !== 'false') void import('./store/mockReviewer').then(m => m.startMockReviewer());

// 잠깐 끊긴 회선이면 한 번 더, 배포로 옛 파일이 사라졌으면 한 번 새로고침 (lib/lazyComponent와 같은 규칙)
loadApp()
  .catch(e => (navigator.onLine ? new Promise(r => setTimeout(r, 700)).then(loadApp) : Promise.reject(e)))
  .then(Root => {
    createRoot(root).render(
      <StrictMode>
        <Root />
      </StrictMode>,
    );
  })
  .catch(e => {
    if (navigator.onLine && isChunkError(e) && claimChunkReload()) { window.location.reload(); return; }
    console.error('[studio] 앱을 불러오지 못했어요', e);
    // 로고 아래 막대 대신 안내 한 줄과 새로고침 버튼 (스타일은 index.html의 첫 화면)
    const boot = root.querySelector('.aq-boot');
    if (!boot) return;
    boot.setAttribute('aria-label', '화면을 불러오지 못했어요');
    boot.querySelector('.aq-boot-bar')?.remove();
    const msg = document.createElement('p');
    msg.textContent = '화면을 불러오지 못했어요. 인터넷 연결을 확인하고 새로고침해 주세요.';
    msg.style.cssText = 'margin:0;font-size:14px;color:#5D687E;text-align:center';
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.textContent = '새로고침';
    btn.style.cssText = 'height:40px;padding:0 18px;border:0;border-radius:12px;background:#3B63F3;color:#fff;font:inherit;font-weight:700';
    btn.onclick = () => window.location.reload();
    boot.append(msg, btn);
  });
