import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';

const root = document.getElementById('root');
if (!root) throw new Error('#root 요소를 찾을 수 없어요.');

// iOS Safari는 터치 리스너가 있어야 :active(누르는 순간 반응)를 적용한다
document.addEventListener('touchstart', () => {}, { passive: true });

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
