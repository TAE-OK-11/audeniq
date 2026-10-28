/// <reference types="vitest/config" />
import { defineConfig, loadEnv } from 'vite';
import react from '@vitejs/plugin-react';
import { fileURLToPath } from 'node:url';
import { retainAssets } from './build/retain-assets';
import { compactCss, purgeCss } from './build/purge-css';
import { hoverGuard } from './build/hover-guard';

const src = (p: string) => fileURLToPath(new URL(p, import.meta.url));

// CloudFront + Rust origin production build: `vite build --mode edge` → web/studio/edge-dist/connected
// (저장소의 public/connected는 체험(목) 모드 빌드)
const API_TARGET = process.env.AUDENIQ_API ?? 'http://127.0.0.1:8080';
const SERVICE_SECRET = process.env.EDGE_SERVICE_SECRET;

export default defineConfig(({ command, mode }) => {
  const env = loadEnv(mode, process.cwd(), 'VITE_');
  if (command === 'build' && mode === 'edge' && env.VITE_API_BASE?.trim()) {
    throw new Error('Studio production API requests must use the same CloudFront origin; remove VITE_API_BASE');
  }
  return ({
  base: '/',
  plugins: [react(), retainAssets()],
  css: {
    postcss: {
      // :hover는 마우스 환경에서만 (휴대폰 sticky hover 방지) — 개발·빌드 모두 적용해 화면이 같게 한다
      // 빌드에서만 미사용 CSS 제거 (개발 중에는 새 클래스를 바로 쓸 수 있도록 전체 유지)
      plugins: command === 'build'
        ? [hoverGuard(), purgeCss({ content: [src('./src'), src('./index.html')] }), compactCss()]
        : [hoverGuard()],
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
      '/api': {
        target: API_TARGET,
        headers: SERVICE_SECRET ? { 'x-audeniq-service': SERVICE_SECRET } : undefined,
        configure: proxy => {
          // 브라우저가 보낸 서비스 신원·IP 헤더는 엣지와 같이 버린다
          proxy.on('proxyReq', req => {
            for (const name of req.getHeaderNames()) {
              if (name.toLowerCase().startsWith('x-audeniq-')) req.removeHeader(name);
            }
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
  });
});
