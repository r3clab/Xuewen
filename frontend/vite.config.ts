/// <reference types="vitest/config" />
import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { brotliCompressSync, constants, gzipSync } from 'node:zlib';
import { defineConfig, type Plugin } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';

const src = (p: string) => fileURLToPath(new URL(`./src/${p}`, import.meta.url));

/** Writes a `.br` and a `.gz` beside every compressible output file, which
 *  the server (src/web/assets.rs) hands to clients that accept them — so
 *  nothing is compressed per request. A sibling that would not be smaller
 *  is skipped, and the server then sends the file as is. */
function precompress(): Plugin {
  const compressible = /\.(?:css|html|js|json|mjs|svg|txt|wasm|xml)$/;
  // Below this, compression framing costs more than it saves.
  const minBytes = 1024;
  return {
    name: 'xuewen:precompress',
    apply: 'build',
    writeBundle({ dir }) {
      if (dir === undefined) throw new Error('precompress: the bundle has no output directory');
      for (const entry of readdirSync(dir, { recursive: true, withFileTypes: true })) {
        if (!entry.isFile() || !compressible.test(entry.name)) continue;
        const file = join(entry.parentPath, entry.name);
        const raw = readFileSync(file);
        if (raw.length < minBytes) continue;
        const siblings: [string, Buffer][] = [
          [
            '.br',
            brotliCompressSync(raw, {
              params: {
                // Not the maximum, 11: on the 7.9 MB PDFium wasm that takes
                // ~34 s per build for a 13% smaller file, where 9 takes ~1 s.
                [constants.BROTLI_PARAM_QUALITY]: 9,
                [constants.BROTLI_PARAM_SIZE_HINT]: raw.length,
              },
            }),
          ],
          ['.gz', gzipSync(raw, { level: constants.Z_BEST_COMPRESSION })],
        ];
        for (const [suffix, packed] of siblings) {
          if (packed.length < raw.length) writeFileSync(file + suffix, packed);
        }
      }
    },
  };
}

/** @embedpdf/engine imports three fallbacks dynamically, so a bundler emits
 *  them however the engine is configured, and they would ship inside the
 *  Rust binary: PDFium in the page's own thread (for a browser with no
 *  Worker), the worker as a string (for scripts on another origin than the
 *  page, which start it from a blob: URL), and the Hebrew/Arabic fallback
 *  fonts (`defaultFallbackFonts: false` in reader/pdf/engine.ts). None
 *  applies here — the backend serves the page and its scripts from one
 *  origin — so the build swaps each for a module that refuses to load. */
function leaveOutEngineFallbacks(): Plugin {
  const unused = (source: string, importer: string | undefined) =>
    source === '@embedpdf/default-fallback-fonts/library' ||
    (importer?.includes('/@embedpdf/engine/') === true &&
      (source === '#inline-transport' || /^\.\/embedpdf-worker\.source-[\w-]+\.js$/.test(source)));
  const prefix = '\0engine-fallback:';
  return {
    name: 'xuewen:leave-out-engine-fallbacks',
    apply: 'build',
    enforce: 'pre',
    resolveId(source, importer) {
      return unused(source, importer) ? prefix + source : null;
    },
    load(id) {
      if (!id.startsWith(prefix)) return null;
      return `throw new Error(${JSON.stringify(`left out of this build: ${id.slice(prefix.length)}`)});`;
    },
  };
}

export default defineConfig({
  plugins: [svelte(), tailwindcss(), leaveOutEngineFallbacks(), precompress()],
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    // The one chunk over the default 500 kB is the reader's viewer (~815 kB,
    // EmbedPDF's runtime and plugins), which loads lazily with the reader and
    // never on the library view; the PDFium worker and wasm are assets, which
    // this check does not count. The limit sits just above the viewer, still
    // catching a genuine regression.
    chunkSizeWarningLimit: 850,
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
