import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { fileURLToPath } from 'node:url';
import { retainAssets } from '../studio/app/build/retain-assets';
import { compactCss, purgeCss } from './build/purge-css';

const src = (p: string) => fileURLToPath(new URL(p, import.meta.url));

// 로컬 백엔드 연동: `EDGE_SERVICE_SECRET=... AUDENIQ_API=http://127.0.0.1:8080 bun run dev`
// (백엔드 APP_ORIGIN은 http://localhost:5174 로 맞춘다)
const API_TARGET = process.env.AUDENIQ_API ?? 'http://127.0.0.1:8080';
const SERVICE_SECRET = process.env.EDGE_SERVICE_SECRET;

export default defineConfig(({ command }) => ({
  base: '/',
  plugins: [react(), retainAssets()],
  css: {
    postcss: {
      // 스튜디오와 같은 디자인 CSS를 그대로 가져오므로, 빌드 때 이 앱이 안 쓰는 규칙은 걷어낸다
      plugins: command === 'build'
        ? [purgeCss({ content: [src('./src'), src('./index.html')] }), compactCss()]
        : [],
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    target: 'baseline-widely-available',
    // 스튜디오와 같은 이유로 Lightning CSS 압축은 끈다 (!important 병합 오류)
    cssMinify: false,
    modulePreload: { polyfill: false },
    reportCompressedSize: false,
  },
  server: {
    port: 5174,
    proxy: {
      '/api': {
        target: API_TARGET,
        configure: proxy => {
          proxy.on('proxyReq', req => {
            req.removeHeader('x-audeniq-client-ip');
            if (SERVICE_SECRET) req.setHeader('x-audeniq-service', SERVICE_SECRET);
            else req.removeHeader('x-audeniq-service');
          });
        },
      },
    },
  },
}));
