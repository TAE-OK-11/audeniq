import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { initPressFeedback } from './lib/press';
import { initRowNav } from './lib/rowNav';

const root = document.getElementById('root');
if (!root) throw new Error('#root 요소를 찾을 수 없어요.');

// iOS Safari는 터치 리스너가 있어야 :active(누르는 순간 반응)를 적용한다
document.addEventListener('touchstart', () => {}, { passive: true });
// 짧게 톡 눌러도 눌린 모습이 보이게 (.is-pressed)
initPressFeedback();
// 탭·칩 줄: 고른 칸이 줄 밖이면 보이게, 화살표 키로 옮기기
initRowNav();

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
