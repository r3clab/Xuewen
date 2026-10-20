// @vitest-environment node
import { describe, expect, it } from 'vitest';

const sources = import.meta.glob<string>(['/src/**/*.ts', '/src/**/*.svelte', '!/src/**/*.d.ts'], {
  query: '?raw',
  import: 'default',
  eager: true,
});
const assets = Object.keys(import.meta.glob(['/src/**', '!/src/**/*.ts', '!/src/**/*.svelte']));
const files = new Set([...Object.keys(sources), ...assets]);

const ALIASES: Record<string, string> = { $app: '/src/app', $features: '/src/features', $shared: '/src/shared' };

type Layer = 'L0' | 'L1' | 'L2' | 'L3' | 'L4';

const LAYERS: Layer[] = ['L0', 'L1', 'L2', 'L3', 'L4'];
const L1_CROSS_FEATURE = new Set(['identify->library', 'importing->library', 'reader->library']);
const L3_CROSS_FEATURE = new Set(['reader->library']);
const APP_ACTIONS = '/src/app/actions.ts';
const TEST_SETUP = '/src/test-setup.ts';

const IMPORT_FROM = /^[ \t]*import\s+(type\s+)?[\w$\s{},*]*?\s*from\s*['"]([^'"]+)['"]/gm;
const EXPORT_FROM = /^[ \t]*export\s+(type\s+)?(?:\*(?:\s+as\s+[\w$]+)?|\{[\w$\s,]*\})\s*from\s*['"]([^'"]+)['"]/gm;
const SIDE_EFFECT = /^[ \t]*import\s*['"]([^'"]+)['"]/gm;
const SCRIPT = /<script\b[^>]*>([\s\S]*?)<\/script>/g;

interface Module {
  imports: string[];
  packages: string[];
  unresolved: string[];
  local: { spec: string; to: string }[];
}

function scripts(file: string, source: string): string {
  return file.endsWith('.svelte') ? [...source.matchAll(SCRIPT)].map((m) => m[1]).join('\n') : source;
}

function specifiers(code: string): { spec: string; typeOnly: boolean }[] {
  const specs: { spec: string; typeOnly: boolean }[] = [];
  for (const re of [IMPORT_FROM, EXPORT_FROM]) {
    for (const m of code.matchAll(re)) specs.push({ spec: m[2], typeOnly: Boolean(m[1]) });
  }
  for (const m of code.matchAll(SIDE_EFFECT)) specs.push({ spec: m[1], typeOnly: false });
  return specs;
}

function folder(file: string): string {
  return file.slice(0, file.lastIndexOf('/'));
}

function join(dir: string, rel: string): string {
  const parts = dir.split('/');
  for (const seg of rel.split('/')) {
    if (seg === '..') parts.pop();
    else if (seg !== '.') parts.push(seg);
  }
  return parts.join('/');
}

function target(from: string, spec: string): string | null {
  const path = spec.split('?')[0];
  const alias = Object.keys(ALIASES).find((a) => path === a || path.startsWith(`${a}/`));
  let base: string;
  if (alias) base = ALIASES[alias] + path.slice(alias.length);
  else if (path.startsWith('.')) base = join(folder(from), path);
  else return null;
  return [base, `${base}.ts`, `${base}.js`, `${base}/index.ts`].find((p) => files.has(p)) ?? base;
}

function buildGraph(): Map<string, Module> {
  const graph = new Map<string, Module>();
  for (const [file, source] of Object.entries(sources)) {
    const mod: Module = { imports: [], packages: [], unresolved: [], local: [] };
    for (const { spec, typeOnly } of specifiers(scripts(file, source))) {
      const to = target(file, spec);
      if (to !== null) mod.local.push({ spec, to });
      if (typeOnly) continue;
      if (to === null) mod.packages.push(spec);
      else if (files.has(to)) mod.imports.push(to);
      else mod.unresolved.push(spec);
    }
    graph.set(file, mod);
  }
  return graph;
}

function cycles(graph: Map<string, Module>): string[][] {
  const index = new Map<string, number>();
  const low = new Map<string, number>();
  const stack: string[] = [];
  const onStack = new Set<string>();
  const found: string[][] = [];
  const visit = (v: string) => {
    index.set(v, index.size);
    low.set(v, index.get(v)!);
    stack.push(v);
    onStack.add(v);
    for (const w of graph.get(v)?.imports ?? []) {
      if (!index.has(w)) {
        visit(w);
        low.set(v, Math.min(low.get(v)!, low.get(w)!));
      } else if (onStack.has(w)) {
        low.set(v, Math.min(low.get(v)!, index.get(w)!));
      }
    }
    if (low.get(v) !== index.get(v)) return;
    const scc: string[] = [];
    let w: string;
    do {
      w = stack.pop()!;
      onStack.delete(w);
      scc.push(w);
    } while (w !== v);
    if (scc.length > 1 || graph.get(v)?.imports.includes(v)) found.push(scc.map((f) => f.replace(/^\/src\//, '')).sort());
  };
  for (const v of graph.keys()) if (!index.has(v)) visit(v);
  return found.sort((a, b) => a[0].localeCompare(b[0]));
}

function closure(graph: Map<string, Module>, entry: string): Set<string> {
  const seen = new Set<string>([entry]);
  const queue = [entry];
  while (queue.length) {
    for (const next of graph.get(queue.pop()!)?.imports ?? []) {
      if (!seen.has(next)) {
        seen.add(next);
        queue.push(next);
      }
    }
  }
  return seen;
}

function inComponentLayer(file: string): boolean {
  const subject = file.replace(/\.test\.ts$/, '.svelte');
  return subject.endsWith('.svelte') && files.has(subject);
}

function layerOf(file: string): Layer | null {
  const component = inComponentLayer(file);
  if (file.startsWith('/src/shared/')) return 'L0';
  if (file.startsWith('/src/features/')) return component ? 'L3' : 'L1';
  if (file.startsWith('/src/app/')) return component ? 'L4' : 'L2';
  return file === '/src/main.ts' ? 'L4' : null;
}

function sameFeatureOr(crossings: Set<string>, from: string, to: string): boolean {
  const [a, b] = [from, to].map((file) => file.split('/')[3]);
  return a === b || crossings.has(`${a}->${b}`);
}

const MAY_IMPORT: Record<Layer, (from: string, to: string, layer: Layer | null) => boolean> = {
  L0: (_from, _to, layer) => layer === 'L0',
  L1: (from, to, layer) =>
    !to.endsWith('.svelte') && (layer === 'L0' || (layer === 'L1' && sameFeatureOr(L1_CROSS_FEATURE, from, to))),
  L2: (_from, to, layer) => !to.endsWith('.svelte') && (layer === 'L0' || layer === 'L1' || layer === 'L2'),
  L3: (from, to, layer) =>
    layer === 'L0' || layer === 'L1' || to === APP_ACTIONS || (layer === 'L3' && sameFeatureOr(L3_CROSS_FEATURE, from, to)),
  L4: () => true,
};

describe('module graph', () => {
  const graph = buildGraph();

  it('resolves every relative and alias import', () => {
    const unresolved = [...graph].flatMap(([file, mod]) => mod.unresolved.map((spec) => `${file}: ${spec}`));
    expect(unresolved).toEqual([]);
  });

  it('has no runtime import cycles', () => {
    expect(cycles(graph)).toEqual([]);
  });

  it('keeps every @embedpdf importer out of the eager closure of main.ts', () => {
    const eager = closure(graph, '/src/main.ts');
    expect(eager).toContain('/src/features/reader/annotations/commands.ts');
    const engine = [...graph].filter(([, mod]) => mod.packages.some((p) => p.startsWith('@embedpdf/'))).map(([file]) => file);
    expect(engine.length).toBeGreaterThan(0);
    expect(engine).toContain('/src/features/reader/pdf/engine.ts');
    expect(engine.filter((file) => eager.has(file))).toEqual([]);
  });

  it('places every source file in a layer and every layer holds files', () => {
    const layered = [...graph.keys()].filter((file) => file !== TEST_SETUP);
    expect(layered.filter((file) => layerOf(file) === null)).toEqual([]);
    expect(new Set(layered.map(layerOf))).toEqual(new Set(LAYERS));
  });

  it('keeps every runtime import inside the layer rules', () => {
    const checked = new Map<Layer, number>();
    const violations: string[] = [];
    for (const [from, mod] of graph) {
      const layer = layerOf(from);
      if (layer === null) continue;
      for (const to of mod.imports) {
        const toLayer = layerOf(to);
        checked.set(layer, (checked.get(layer) ?? 0) + 1);
        if (!MAY_IMPORT[layer](from, to, toLayer)) violations.push(`${from} (${layer}) -> ${to} (${toLayer})`);
      }
    }
    expect(violations).toEqual([]);
    expect(LAYERS.filter((layer) => !checked.get(layer))).toEqual([]);
  });

  it('imports its own folder with ./ and every other folder with an alias', () => {
    const local = [...graph].flatMap(([file, mod]) => mod.local.map(({ spec, to }) => ({ file, spec, to })));
    expect(new Set(local.map(({ spec }) => spec.startsWith('.')))).toEqual(new Set([true, false]));
    const misnamed = local
      .filter(({ file, spec, to }) => spec.startsWith('.') !== (folder(to) === folder(file)))
      .map(({ file, spec }) => `${file}: ${spec}`);
    expect(misnamed).toEqual([]);
  });
});
