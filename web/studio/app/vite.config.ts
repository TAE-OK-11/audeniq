/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { fileURLToPath } from 'node:url';
import { purgeCss } from './build/purge-css';

const src = (p: string) => fileURLToPath(new URL(p, import.meta.url));

export default defineConfig(({ command }) => ({
  base: '/connected/',
  plugins: [react()],
  css: {
    postcss: {
      // 빌드에서만 미사용 CSS 제거 (개발 중에는 새 클래스를 바로 쓸 수 있도록 전체 유지)
      plugins: command === 'build'
        ? [purgeCss({ content: [src('./src'), src('./index.html')] })]
        : [],
    },
  },
  build: {
    outDir: '../public/connected',
    emptyOutDir: true,
    target: 'baseline-widely-available',
    // 대상 브라우저가 모두 modulepreload를 지원하므로 폴리필 제거
    modulePreload: { polyfill: false },
    reportCompressedSize: false,
  },
  server: {
    proxy: {
      '/api': 'http://localhost:8080',
    },
  },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.ts', 'src/**/*.test.tsx', 'build/**/*.test.ts'],
  },
}));
