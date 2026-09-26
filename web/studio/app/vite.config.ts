/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  base: '/connected/',
  plugins: [react()],
  build: {
    outDir: '../public/connected',
    emptyOutDir: true,
    target: 'baseline-widely-available',
    cssMinify: false, // esbuild가 @media를 버리는 버그 우회
  },
  server: {
    proxy: {
      '/api': 'http://localhost:8080',
    },
  },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.ts', 'src/**/*.test.tsx'],
  },
});
