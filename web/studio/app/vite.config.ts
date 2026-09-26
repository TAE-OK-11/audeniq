/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { fileURLToPath } from 'node:url';
import { compactCss, purgeCss } from './build/purge-css';

const src = (p: string) => fileURLToPath(new URL(p, import.meta.url));

// 엣지 Worker 배포용 서버 연결 빌드는 `vite build --mode edge` → web/studio/edge-dist/connected
// (저장소의 public/connected는 체험(목) 모드 빌드)
const API_TARGET = process.env.AUDENIQ_API ?? 'http://127.0.0.1:8080';
const SERVICE_SECRET = process.env.EDGE_SERVICE_SECRET;
// 공지·이벤트는 운영에서 엣지 Worker(D1)가 서빙한다. 로컬에서는 `wrangler dev` 주소를 지정하면 그쪽으로 보낸다.
const CONTENT_TARGET = process.env.EDGE_CONTENT_URL;

export default defineConfig(({ command, mode }) => ({
  base: '/',
  plugins: [react()],
  css: {
    postcss: {
      // 빌드에서만 미사용 CSS 제거 (개발 중에는 새 클래스를 바로 쓸 수 있도록 전체 유지)
      plugins: command === 'build'
        ? [purgeCss({ content: [src('./src'), src('./index.html')] }), compactCss()]
        : [],
    },
  },
  build: {
    outDir: mode === 'edge' ? '../edge-dist' : '../public',
    emptyOutDir: true,
    target: 'baseline-widely-available',
    // Lightning CSS 압축기가 !important 규칙을 잘못 병합하므로 끄고 compactCss()로 안전하게 압축
    cssMinify: false,
    // 대상 브라우저가 모두 modulepreload를 지원하므로 폴리필 제거
    modulePreload: { polyfill: false },
    reportCompressedSize: false,
  },
  server: {
    // 로컬 백엔드 연동: 엣지 Worker처럼 서비스 비밀 헤더를 붙여 API로 전달한다.
    // 실행: EDGE_SERVICE_SECRET=... bun run dev:api  (APP_ORIGIN=http://localhost:5173)
    proxy: {
      ...(CONTENT_TARGET ? { '^/api/(notices|events)': { target: CONTENT_TARGET, changeOrigin: true } } : {}),
      '/api': {
        target: API_TARGET,
        headers: SERVICE_SECRET ? { 'x-audeniq-service': SERVICE_SECRET } : undefined,
        configure: proxy => {
          // 브라우저가 보낸 서비스 신원·IP 헤더는 엣지와 같이 버린다
          proxy.on('proxyReq', req => {
            req.removeHeader('x-audeniq-client-ip');
            if (SERVICE_SECRET) req.setHeader('x-audeniq-service', SERVICE_SECRET);
            else req.removeHeader('x-audeniq-service');
          });
        },
      },
    },
  },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.ts', 'src/**/*.test.tsx', 'build/**/*.test.ts'],
  },
}));
