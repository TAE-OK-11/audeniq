import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  base: '/connected/',
  plugins: [react()],
  build: {
    outDir: '../public/connected',
    emptyOutDir: true,
    cssMinify: false,
  },
  server: {
    proxy: {
      '/api': 'http://localhost:8080',
    },
  },
});
