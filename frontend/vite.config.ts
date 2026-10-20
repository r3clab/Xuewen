/// <reference types="vitest/config" />
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';

const src = (p: string) => fileURLToPath(new URL(`./src/${p}`, import.meta.url));

export default defineConfig({
  plugins: [svelte(), tailwindcss()],
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    // The only chunk over the default 500 kB is @embedpdf's PDFium worker
    // engine (~690 kB), which runs off the main thread and loads lazily with
    // the reader — not on the initial library view. Raise the limit so that
    // inherent-and-deferred chunk stops tripping the warning, while still
    // catching a genuine regression above 700 kB.
    chunkSizeWarningLimit: 700,
  },
  server: {
    proxy: {
      '/api': 'http://127.0.0.1:8080',
      '/papers': 'http://127.0.0.1:8080',
    },
  },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/test-setup.ts'],
  },
  resolve: {
    alias: { $app: src('app'), $features: src('features'), $shared: src('shared') },
    conditions: process.env.VITEST ? ['browser'] : undefined,
  },
});
