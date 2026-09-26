// 없는 주소 — Workers Static Assets가 404 상태로 이 페이지를 준다 (not_found_handling: 404-page)
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

function NotFound() {
  return (
    <main style={{ fontFamily: 'system-ui,sans-serif', maxWidth: '48rem', margin: '5rem auto', padding: '1rem', color: '#171b29' }}>
      <h1>페이지를 찾을 수 없어요.</h1>
      <p>요청한 주소를 다시 확인해 주세요.</p>
      <a href="/">설문으로 돌아가기</a>
    </main>
  );
}

createRoot(document.getElementById('root')!).render(<StrictMode><NotFound /></StrictMode>);
