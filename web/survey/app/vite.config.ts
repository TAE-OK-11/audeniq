import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { fileURLToPath } from 'node:url';

const src = (p: string) => fileURLToPath(new URL(p, import.meta.url));

// 빌드 결과는 ../public (Workers Static Assets가 그대로 서빙, 페이지 조회에 Worker 실행 없음)
export default defineConfig({
  base: '/',
  plugins: [react()],
  build: {
    outDir: '../public',
    emptyOutDir: true,
    target: 'baseline-widely-available',
    modulePreload: { polyfill: false },
    reportCompressedSize: false,
    rollupOptions: { input: { index: src('./index.html'), notFound: src('./404.html') } },
  },
  server: {
    // 로컬: bunx wrangler dev(:8787)의 /api로 제출 (선택)
    proxy: process.env.SURVEY_API ? { '/api': { target: process.env.SURVEY_API, changeOrigin: false } } : undefined,
  },
});
