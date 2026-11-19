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
    // The chunks over the default 500 kB are all @embedpdf's, none on the
    // initial library view: the viewer (~815 kB), which loads lazily with the
    // reader, and two engine fallbacks of ~1.65 MB each that this app never
    // fetches — PDFium in the main thread, for a page with no Worker, and the
    // worker's source, for a worker that has to start from a blob: URL. The
    // engine imports both dynamically, so the bundler emits them regardless.
    // The limit sits just above them, still catching a genuine regression.
    chunkSizeWarningLimit: 1700,
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
