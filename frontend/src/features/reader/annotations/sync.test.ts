import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Annotation } from '$shared/api/generated/Annotation';
import type { NewAnnotation } from '$shared/api/generated/NewAnnotation';

const server: Record<string, Record<string, Annotation>> = {};
let putFails: string | null = null;
let listFails: string | null = null;

/// What a payload looks like once it has been through the backend: the Rust
/// side parses it into a `serde_json::Value`, whose map is a BTreeMap, so every
/// object comes back with its keys alphabetized rather than in the order the
/// plugin wrote them. Every stored payload below goes through this, so the
/// "nothing changed" checks are made against a realistic echo.
function serverJson<T>(v: T): T {
  if (v === null || typeof v !== 'object') return v;
  if (Array.isArray(v)) return v.map(serverJson) as T;
  return Object.fromEntries(
    Object.entries(v as Record<string, unknown>)
      .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
      .map(([k, value]) => [k, serverJson(value)]),
  ) as T;
}

vi.mock(import('./api'), () => ({
  listAnnotations: vi.fn(async (paperId: string) => {
    if (listFails) throw new Error(listFails);
    return Object.values(server[paperId] ?? {});
  }),
  putAnnotation: vi.fn(async (paperId: string, id: string, b: NewAnnotation) => {
    if (putFails) throw new Error(putFails);
    const saved: Annotation = {
      paper_id: paperId,
      id,
      ...b,
      payload: serverJson(b.payload),
      created_at: '2026-08-14T00:00:00Z',
      updated_at: '2026-08-14T01:00:00Z',
    };
    (server[paperId] ??= {})[id] = saved;
    return saved;
  }),
  deleteAnnotation: vi.fn(async (paperId: string, id: string) => {
    delete server[paperId]?.[id];
  }),
}));

import * as api from './api';
import { annotationKey, type PageRef } from '@embedpdf/svelte/runtime';
import type { AnnotationRef } from '@embedpdf/svelte/annotation';
import { createAnnotationSync, type SyncScope } from './sync';
import { annotationList, annotations } from './store.svelte';
import { colorHex } from './palette';
import { RESTORE_OPTIONS, toWire, type MarkData } from './adapter';

const PAGE_COUNT = 6;

function pageRef(index: number): PageRef {
  return { kind: 'objectNumber', objectNumber: 100 + index };
}

const QUAD = {
  upperLeft: { x: 0, y: 0 },
  upperRight: { x: 10, y: 0 },
  lowerLeft: { x: 0, y: 10 },
  lowerRight: { x: 10, y: 10 },
};

/// A highlight on page 1 the way the engine reads one back. Its ref is only
/// the object it was written as; the harness gives each mark a fresh one.
function mark(nm: string, over: Partial<MarkData> & Record<string, unknown> = {}): MarkData {
  return {
    subtype: 'highlight',
    ref: { kind: 'objectNumber', page: pageRef(1), objectNumber: 1 },
    nm,
    color: colorHex('amber'),
    contents: null,
    quadPoints: [QUAD],
    ...over,
  };
}

function stored(paperId: string, id: string, pageIndex: number, payload: unknown, over: Partial<NewAnnotation> = {}): void {
  (server[paperId] ??= {})[id] = {
    paper_id: paperId,
    id,
    page_index: pageIndex,
    kind: 'highlight',
    color: 'amber',
    quoted_text: 'quoted',
    note: null,
    ...over,
    payload: serverJson(payload),
    created_at: '2026-08-14T00:00:00Z',
    updated_at: '2026-08-14T00:00:00Z',
  };
}

function seed(paperId: string, a: MarkData): void {
  const wire = toWire(a, a.ref.page.objectNumber - 100, 'quoted')!;
  stored(paperId, a.nm!, wire.page_index, wire.payload, wire);
}

type Listener<E> = (event: E) => void;

/// A stand-in for one document's annotation capability: the marks in it, and
/// its events, fired the way the engine fires them — a create and an import
/// fire onCreated, an edit onUpdated, a delete onDeleted, and an undo or redo
/// that brings a mark back or takes one away only onResynced.
function harness(paperId = 'p1') {
  const doc = new Map<string, MarkData>();
  let objects = 500;
  const listeners = {
    created: new Set<Listener<{ annotation: MarkData }>>(),
    updated: new Set<Listener<{ annotation: MarkData }>>(),
    deleted: new Set<Listener<{ refs: readonly AnnotationRef[] }>>(),
    resynced: new Set<Listener<{ pages: readonly PageRef[] | 'all' }>>(),
  };
  const on =
    <E,>(set: Set<Listener<E>>) =>
    (listener: Listener<E>) => {
      set.add(listener);
      return () => void set.delete(listener);
    };
  const fire = <E,>(set: Set<Listener<E>>, event: E) => {
    for (const listener of set) listener(event);
  };

  /// Writes a mark into the document as a new object.
  function put(a: MarkData, page: PageRef = a.ref.page): MarkData {
    const written = { ...a, ref: { kind: 'objectNumber', page, objectNumber: ++objects } } as const;
    doc.set(annotationKey(written.ref), written);
    return written;
  }

  const imports: Parameters<SyncScope['import']>[] = [];
  const creates: Parameters<SyncScope['create']>[] = [];
  const scope: SyncScope = {
    get: (ref) => doc.get(annotationKey(ref)) ?? null,
    list: () => [...doc.values()],
    import: async (bundle, options) => {
      imports.push([bundle, options]);
      const out = bundle.items.map((item) => put(item.data));
      for (const annotation of out) fire(listeners.created, { annotation });
      return { annotations: out };
    },
    create: async (page, draft) => {
      creates.push([page, draft]);
      const annotation = put({ ...draft, ref: { kind: 'objectNumber', page, objectNumber: 0 } }, page);
      fire(listeners.created, { annotation });
      return { annotation };
    },
    onCreated: on(listeners.created),
    onUpdated: on(listeners.updated),
    onDeleted: on(listeners.deleted),
    onResynced: on(listeners.resynced),
  };

  const errors: string[] = [];
  const clearHistory = vi.fn();
  const textUnder = vi.fn(async (_page: PageRef, _quads: unknown) => 'quoted');
  const sync = createAnnotationSync({
    paperId,
    scope,
    pages: {
      list: () => Array.from({ length: PAGE_COUNT }, (_, index) => ({ index, ref: pageRef(index), size: { width: 612, height: 792 } })),
      indexOf: (ref) => (ref.objectNumber >= 100 && ref.objectNumber < 100 + PAGE_COUNT ? ref.objectNumber - 100 : -1),
      textUnder,
    },
    clearHistory,
    onError: (m) => errors.push(m),
    debounceMs: 20,
  });

  const find = (nm: string): MarkData => {
    const found = [...doc.values()].find((a) => a.nm === nm);
    if (!found) throw new Error(`no mark ${nm} in the document`);
    return found;
  };

  return {
    sync,
    imports,
    creates,
    errors,
    clearHistory,
    textUnder,
    find,
    listening: () => Object.values(listeners).reduce((n, set) => n + set.size, 0),
    /// The user draws a mark.
    draw(a: MarkData): MarkData {
      const annotation = put(a);
      fire(listeners.created, { annotation });
      return annotation;
    },
    /// A mark the PDF came with, never created or imported by us.
    foreign(a: MarkData): MarkData {
      return put(a);
    },
    edit(nm: string, patch: Partial<MarkData>): void {
      const annotation = { ...find(nm), ...patch };
      doc.set(annotationKey(annotation.ref), annotation);
      fire(listeners.updated, { annotation });
    },
    remove(nm: string): void {
      const { ref } = find(nm);
      doc.delete(annotationKey(ref));
      fire(listeners.deleted, { refs: [ref] });
    },
    /// An undo or redo that brings a mark back as a new object.
    bringBack(a: MarkData, pages: readonly PageRef[] | 'all' = [a.ref.page]): void {
      put(a);
      fire(listeners.resynced, { pages });
    },
    /// An undo or redo that takes a mark away.
    takeAway(nm: string): void {
      const { ref } = find(nm);
      doc.delete(annotationKey(ref));
      fire(listeners.resynced, { pages: [ref.page] });
    },
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  for (const k of Object.keys(server)) delete server[k];
  putFails = null;
  listFails = null;
  annotations.byPaper = {};
  annotations.loaded = {};
  annotations.error = {};
});

describe('start', () => {
  it('restores the stored marks into the document under their own names', async () => {
    seed('p1', mark('a1'));
    seed('p1', mark('a2', { ref: { kind: 'objectNumber', page: pageRef(4), objectNumber: 1 } }));
    const h = harness();
    await h.sync.start();
    expect(h.imports).toHaveLength(1);
    const [bundle, options] = h.imports[0];
    expect(options).toEqual(RESTORE_OPTIONS);
    expect(bundle.items.map((item) => item.data.nm).sort()).toEqual(['a1', 'a2']);
    expect(h.find('a2').ref.page).toEqual(pageRef(4));
  });

  it('does not rewrite a mark that came back unchanged', async () => {
    seed('p1', mark('a1'));
    const h = harness();
    await h.sync.start();
    await h.sync.flush();
    expect(api.putAnnotation).not.toHaveBeenCalled();
  });

  it('skips a row whose payload the backend could not parse', async () => {
    seed('p1', mark('a1'));
    server['p1']['a1'].payload = null;
    const h = harness();
    await h.sync.start();
    expect(h.imports).toEqual([]);
    // The row is still in the store, so the panel can list it.
    expect(annotationList('p1')).toHaveLength(1);
  });

  it('imports nothing, quietly, for a paper with no marks', async () => {
    const h = harness();
    await h.sync.start();
    expect(h.imports).toEqual([]);
    expect(h.clearHistory).not.toHaveBeenCalled();
    expect(api.putAnnotation).not.toHaveBeenCalled();
  });

  it('skips the restore when the sidecar load failed — unknown is not empty', async () => {
    seed('p1', mark('a1'));
    listFails = 'network down';
    const h = harness();
    await h.sync.start();
    expect(h.imports).toEqual([]);
    // Marks drawn this session still save: the subscriptions outlive the bail.
    h.draw(mark('new1'));
    await h.sync.flush();
    expect(api.putAnnotation).toHaveBeenCalledWith('p1', 'new1', expect.anything());
  });

  it('redraws a row the 2.x reader saved, re-saves it in the current form, and leaves no undo step', async () => {
    const rect = { origin: { x: 0, y: 0 }, size: { width: 10, height: 10 } };
    stored('p1', 'old', 2, { annotation: { id: 'old', type: 9, pageIndex: 2, rect, segmentRects: [rect] } });
    const h = harness();
    await h.sync.start();
    await h.sync.flush();
    expect(h.creates).toEqual([[pageRef(2), expect.objectContaining({ subtype: 'highlight', nm: 'old' })]]);
    expect(h.clearHistory).toHaveBeenCalledOnce();
    expect(api.putAnnotation).toHaveBeenCalledWith(
      'p1',
      'old',
      expect.objectContaining({ page_index: 2, payload: { data: expect.objectContaining({ nm: 'old' }) } }),
    );
  });
});

describe('ownership', () => {
  it('saves a mark the user draws, with the passage under it', async () => {
    const h = harness();
    await h.sync.start();
    h.draw(mark('new1', { ref: { kind: 'objectNumber', page: pageRef(3), objectNumber: 1 } }));
    await h.sync.flush();
    expect(h.textUnder).toHaveBeenCalledWith(pageRef(3), [QUAD]);
    expect(api.putAnnotation).toHaveBeenCalledWith(
      'p1',
      'new1',
      expect.objectContaining({ kind: 'highlight', page_index: 3, quoted_text: 'quoted' }),
    );
  });

  it('still saves a mark whose passage cannot be read', async () => {
    const h = harness();
    await h.sync.start();
    h.textUnder.mockRejectedValueOnce(new Error('no text'));
    h.draw(mark('new1'));
    await h.sync.flush();
    expect(api.putAnnotation).toHaveBeenCalledWith('p1', 'new1', expect.objectContaining({ quoted_text: null }));
  });

  it('never adopts a mark that came baked into the PDF', async () => {
    const h = harness();
    await h.sync.start();
    // A foreign annotation is in the document but was never created or
    // imported by us. Editing it must not copy it into the sidecar, or the
    // next load would draw it twice.
    h.foreign(mark('foreign'));
    h.edit('foreign', { color: colorHex('blue') });
    await h.sync.flush();
    expect(api.putAnnotation).not.toHaveBeenCalled();
  });

  it('never deletes the sidecar row for a mark it does not own', async () => {
    const h = harness();
    await h.sync.start();
    h.foreign(mark('foreign'));
    h.remove('foreign');
    await h.sync.flush();
    expect(api.deleteAnnotation).not.toHaveBeenCalled();
  });

  it('owns a mark it restored, so an edit to it persists', async () => {
    seed('p1', mark('a1'));
    const h = harness();
    await h.sync.start();
    h.edit('a1', { color: colorHex('blue') });
    await h.sync.flush();
    expect(api.putAnnotation).toHaveBeenCalledWith('p1', 'a1', expect.objectContaining({ color: 'blue' }));
  });

  it('refuses a subtype outside the whitelist even on a create', async () => {
    const h = harness();
    await h.sync.start();
    h.draw(mark('inky', { subtype: 'ink' }));
    await h.sync.flush();
    expect(api.putAnnotation).not.toHaveBeenCalled();
    // And having refused it, a later edit is still not ours.
    h.edit('inky', { color: colorHex('blue') });
    await h.sync.flush();
    expect(api.putAnnotation).not.toHaveBeenCalled();
  });
});

describe('debouncing', () => {
  it('writes a new mark straight away — a drawn mark should be durable fast', async () => {
    const h = harness();
    await h.sync.start();
    h.draw(mark('new1'));
    await Promise.resolve();
    await Promise.resolve();
    // No flush, no timer advance.
    expect(api.putAnnotation).toHaveBeenCalledTimes(1);
  });

  it('collapses a burst of edits into one write', async () => {
    vi.useFakeTimers();
    const h = harness();
    await h.sync.start();
    h.draw(mark('a1'));
    await h.sync.flush();
    vi.mocked(api.putAnnotation).mockClear();
    for (const note of ['w', 'wo', 'wor', 'worth']) h.edit('a1', { contents: note });
    expect(api.putAnnotation).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(20);
    expect(api.putAnnotation).toHaveBeenCalledTimes(1);
    expect(api.putAnnotation).toHaveBeenCalledWith('p1', 'a1', expect.objectContaining({ note: 'worth' }));
    vi.useRealTimers();
  });

  it('skips a write when nothing actually changed', async () => {
    seed('p1', mark('a1'));
    const h = harness();
    await h.sync.start();
    // Selecting a mark can emit an update that changes nothing.
    h.edit('a1', {});
    await h.sync.flush();
    expect(api.putAnnotation).not.toHaveBeenCalled();
  });

  it('notices a move, which only shows up in the payload', async () => {
    seed('p1', mark('a1'));
    const h = harness();
    await h.sync.start();
    h.edit('a1', { quadPoints: [{ ...QUAD, upperLeft: { x: 5, y: 5 } }] });
    await h.sync.flush();
    expect(api.putAnnotation).toHaveBeenCalledTimes(1);
  });

  it('drops a pending write when the mark is deleted first', async () => {
    vi.useFakeTimers();
    seed('p1', mark('a1'));
    const h = harness();
    await h.sync.start();
    h.edit('a1', { contents: 'half-typed' });
    h.remove('a1');
    await vi.advanceTimersByTimeAsync(50);
    expect(api.deleteAnnotation).toHaveBeenCalledWith('p1', 'a1');
    expect(api.putAnnotation).not.toHaveBeenCalled();
    vi.useRealTimers();
  });
});

describe('deleting', () => {
  it('removes the row and forgets where the mark was', async () => {
    seed('p1', mark('a1'));
    const h = harness();
    await h.sync.start();
    const { ref } = h.find('a1');
    h.remove('a1');
    await h.sync.flush();
    expect(api.deleteAnnotation).toHaveBeenCalledWith('p1', 'a1');
    expect(annotationList('p1')).toHaveLength(0);
    // A second delete event for the same object must not fire again.
    h.foreign({ ...mark('x'), ref });
    h.remove('x');
    await h.sync.flush();
    expect(api.deleteAnnotation).toHaveBeenCalledTimes(1);
  });
});

describe('undo and redo', () => {
  it('writes a mark back when an undo restores it with only a resync', async () => {
    const h = harness();
    await h.sync.start();
    const drawn = h.draw(mark('a1'));
    await h.sync.flush();
    h.remove('a1');
    await h.sync.flush();
    expect(server['p1']['a1']).toBeUndefined();
    h.bringBack(drawn);
    await h.sync.flush();
    expect(server['p1']['a1']).toBeDefined();
    expect(annotationList('p1')).toHaveLength(1);
  });

  it('removes the row when a redo takes the mark away with only a resync', async () => {
    seed('p1', mark('a1'));
    const h = harness();
    await h.sync.start();
    h.takeAway('a1');
    await h.sync.flush();
    expect(api.deleteAnnotation).toHaveBeenCalledWith('p1', 'a1');
  });

  it('follows a resync of the whole document', async () => {
    const h = harness();
    await h.sync.start();
    const drawn = h.draw(mark('a1'));
    await h.sync.flush();
    h.remove('a1');
    h.bringBack(drawn, 'all');
    await h.sync.flush();
    expect(server['p1']['a1']).toBeDefined();
  });

  it('leaves alone what a resync of other pages did not touch', async () => {
    seed('p1', mark('a1'));
    const h = harness();
    await h.sync.start();
    h.bringBack(mark('foreign', { ref: { kind: 'objectNumber', page: pageRef(4), objectNumber: 1 } }));
    await h.sync.flush();
    expect(api.putAnnotation).not.toHaveBeenCalled();
    expect(api.deleteAnnotation).not.toHaveBeenCalled();
  });

  it('restores the row on an undo that lands before the delete settles', async () => {
    const h = harness();
    await h.sync.start();
    const drawn = h.draw(mark('a1'));
    await h.sync.flush();
    // The DELETE is still in flight when the mark comes back, so a save that
    // read the cache eagerly would find the row it is about to remove, call
    // the mark unchanged, and skip the write.
    h.remove('a1');
    h.bringBack(drawn);
    await h.sync.flush();
    expect(server['p1']['a1']).toBeDefined();
    expect(annotationList('p1')).toHaveLength(1);
  });
});

describe('closing the tab', () => {
  it('flushes a pending edit rather than losing it', async () => {
    vi.useFakeTimers();
    seed('p1', mark('a1'));
    const h = harness();
    await h.sync.start();
    h.edit('a1', { contents: 'just typed' });
    await h.sync.destroy(); // before the debounce would have fired
    expect(api.putAnnotation).toHaveBeenCalledWith('p1', 'a1', expect.objectContaining({ note: 'just typed' }));
    vi.useRealTimers();
  });

  it('stops listening', async () => {
    const h = harness();
    await h.sync.start();
    expect(h.listening()).toBe(4);
    await h.sync.destroy();
    expect(h.listening()).toBe(0);
  });

  it('ignores an event arriving while destroy is still flushing', async () => {
    vi.useFakeTimers();
    const h = harness();
    await h.sync.start();
    // Hold the create's write in flight so destroy()'s flush has to wait.
    let release!: () => void;
    vi.mocked(api.putAnnotation).mockImplementationOnce(
      (paperId: string, id: string, b: NewAnnotation) =>
        new Promise((resolve) => {
          release = () =>
            resolve({ paper_id: paperId, id, ...b, created_at: '2026-08-14T00:00:00Z', updated_at: '2026-08-14T01:00:00Z' });
        }),
    );
    h.draw(mark('a1'));
    await vi.advanceTimersByTimeAsync(0);
    const destroyed = h.sync.destroy();
    // The plugin can still deliver events during the flush await (teardown
    // churn, not a user edit). This must not arm a debounce timer that fires
    // a write after the document is closed.
    h.edit('a1', { contents: 'teardown noise' });
    release();
    await destroyed;
    await vi.advanceTimersByTimeAsync(50);
    expect(api.putAnnotation).toHaveBeenCalledTimes(1);
    vi.useRealTimers();
  });
});

describe('when a write fails', () => {
  it('reports once per burst instead of per mark', async () => {
    putFails = 'saving the annotation failed: 500';
    const h = harness();
    await h.sync.start();
    h.draw(mark('a1'));
    h.draw(mark('a2'));
    h.draw(mark('a3'));
    await h.sync.flush();
    expect(h.errors).toEqual(['saving the annotation failed: 500']);
  });

  it('reports again after a write succeeds in between', async () => {
    putFails = 'boom';
    const h = harness();
    await h.sync.start();
    h.draw(mark('a1'));
    await h.sync.flush();
    putFails = null;
    h.draw(mark('a2'));
    await h.sync.flush();
    putFails = 'boom again';
    h.draw(mark('a3'));
    await h.sync.flush();
    expect(h.errors).toEqual(['boom', 'boom again']);
  });

  it('does not throw out of the event handler', async () => {
    putFails = 'boom';
    const h = harness();
    await h.sync.start();
    expect(() => h.draw(mark('a1'))).not.toThrow();
    await expect(h.sync.flush()).resolves.toBeUndefined();
  });
});
