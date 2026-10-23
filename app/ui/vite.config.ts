import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';

export default defineConfig({
  base: './',
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    outDir: 'dist',
    target: 'es2022',
    // Fonts and icons stay as files in dist; nothing is fetched at runtime.
    assetsInlineLimit: 0,
    sourcemap: false,
  },
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts'],
  },
});
