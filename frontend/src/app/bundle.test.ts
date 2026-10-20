// @vitest-environment node
import { build, type Rollup } from 'vite';
import { beforeAll, describe, expect, it } from 'vitest';

declare const process: { env: Record<string, string | undefined> };

const EAGER_JS_BASELINE = 229_415;
const EAGER_JS_BUDGET = Math.ceil(EAGER_JS_BASELINE * 1.1);

function setEnv(name: string, value: string | undefined): void {
  if (value === undefined) delete process.env[name];
  else process.env[name] = value;
}

async function productionChunks(): Promise<Rollup.OutputChunk[]> {
  const root = decodeURIComponent(new URL('../..', import.meta.url).pathname);
  // vitest sets NODE_ENV=test and VITEST, and both change what vite emits;
  // build exactly what `vite build` ships.
  const saved = { NODE_ENV: process.env.NODE_ENV, VITEST: process.env.VITEST };
  setEnv('NODE_ENV', 'production');
  setEnv('VITEST', undefined);
  try {
    const out = await build({ root, configFile: `${root}vite.config.ts`, logLevel: 'silent', build: { write: false } });
    return (Array.isArray(out) ? out : [out])
      .flatMap((o) => (o as Rollup.RollupOutput).output)
      .filter((o): o is Rollup.OutputChunk => o.type === 'chunk');
  } finally {
    setEnv('NODE_ENV', saved.NODE_ENV);
    setEnv('VITEST', saved.VITEST);
  }
}

describe('production bundle', () => {
  let chunks: Rollup.OutputChunk[] = [];
  let entry: Rollup.OutputChunk;
  let eager: Rollup.OutputChunk[] = [];

  beforeAll(async () => {
    chunks = await productionChunks();
    const entries = chunks.filter((c) => c.isEntry);
    expect(entries).toHaveLength(1);
    entry = entries[0];
    const byName = new Map(chunks.map((c) => [c.fileName, c]));
    const seen = new Set([entry.fileName]);
    const queue = [entry.fileName];
    while (queue.length) {
      for (const next of byName.get(queue.pop()!)!.imports) {
        if (!seen.has(next)) {
          seen.add(next);
          queue.push(next);
        }
      }
    }
    eager = chunks.filter((c) => seen.has(c.fileName));
  }, 120_000);

  it('keeps every @embedpdf module out of the eager chunks', () => {
    const engine = chunks.filter((c) => Object.keys(c.modules).some((id) => id.includes('/@embedpdf/')));
    expect(engine.length).toBeGreaterThan(0);
    expect(engine.filter((c) => eager.includes(c)).map((c) => c.fileName)).toEqual([]);
  });

  it('loads the PDF viewer through a dynamic import from the entry', () => {
    const viewer = chunks.find((c) => Object.keys(c.modules).some((id) => id.endsWith('/PdfViewer.svelte')));
    expect(viewer).toBeDefined();
    expect(eager).not.toContain(viewer);
    expect(entry.dynamicImports).toContain(viewer!.fileName);
  });

  it('keeps the eager JS within its byte budget', () => {
    const bytes = eager.reduce((sum, c) => sum + new TextEncoder().encode(c.code).length, 0);
    expect(bytes).toBeLessThanOrEqual(EAGER_JS_BUDGET);
  });
});
