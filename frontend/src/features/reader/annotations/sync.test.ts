import { beforeEach, describe, expect, it, vi } from 'vitest';
import { PdfAnnotationSubtype } from '@embedpdf/models';
import type { PdfAnnotationObject } from '@embedpdf/models';
import type { AnnotationEvent, AnnotationTransferItem } from '@embedpdf/plugin-annotation';
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
import { createAnnotationSync, type SyncScope } from './sync';
import { annotationList, annotations } from './store.svelte';
import { colorHex } from './palette';
import { toWire } from './adapter';

function mark(id: string, over: Partial<PdfAnnotationObject> = {}): PdfAnnotationObject {
  return {
    id,
    type: PdfAnnotationSubtype.HIGHLIGHT,
    pageIndex: 1,
    rect: { origin: { x: 0, y: 0 }, size: { width: 10, height: 10 } },
    color: colorHex('amber'),
    strokeColor: colorHex('amber'),
    custom: { text: 'quoted' },
    ...over,
  } as PdfAnnotationObject;
}

function seed(paperId: string, a: PdfAnnotationObject): void {
  const wire = toWire({ annotation: a })!;
  (server[paperId] ??= {})[a.id] = {
    paper_id: paperId,
    id: a.id,
    ...wire,
    payload: serverJson(wire.payload),
    created_at: '2026-08-14T00:00:00Z',
    updated_at: '2026-08-14T00:00:00Z',
  };
}

/// A stand-in for the plugin's document scope plus its event hook.
function harness(paperId = 'p1', documentId = 'd1') {
  const objects = new Map<string, PdfAnnotationObject>();
  const imported: AnnotationTransferItem[] = [];
  let handler: ((e: AnnotationEvent) => void) | null = null;
  let unsubscribed = false;

  const scope: SyncScope = {
    importAnnotations: (items) => {
      imported.push(...items);
      for (const i of items) objects.set(i.annotation.id, i.annotation);
    },
    getAnnotationById: (id) => {
      const object = objects.get(id);
      return object ? { object } : null;
    },
  };

  const errors: string[] = [];
  const sync = createAnnotationSync({
    paperId,
    documentId,
    scope,
    subscribe: (h) => {
      handler = h;
      return () => {
        unsubscribed = true;
      };
    },
    onError: (m) => errors.push(m),
    debounceMs: 20,
  });

  return {
    sync,
    imported,
    errors,
    objects,
    get unsubscribed() {
      return unsubscribed;
    },
    /// Put an object in the document the way the plugin would, then fire the
    /// matching event.
    emit(e: AnnotationEvent): void {
      if (e.type !== 'loaded') objects.set(e.annotation.id, e.annotation);
      if (e.type === 'delete') objects.delete(e.annotation.id);
      handler?.(e);
    },
  };
}

function created(a: PdfAnnotationObject, documentId = 'd1'): AnnotationEvent {
  return { type: 'create', documentId, annotation: a, pageIndex: a.pageIndex, committed: false };
}
function updated(a: PdfAnnotationObject, documentId = 'd1'): AnnotationEvent {
  return {
    type: 'update',
    documentId,
    annotation: a,
    pageIndex: a.pageIndex,
    patch: {},
    committed: false,
  };
}
function deleted(a: PdfAnnotationObject, documentId = 'd1'): AnnotationEvent {
  return { type: 'delete', documentId, annotation: a, pageIndex: a.pageIndex, committed: false };
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
  it('replays the stored marks into the document', async () => {
    seed('p1', mark('a1'));
    seed('p1', mark('a2', { pageIndex: 4 }));
    const h = harness();
    await h.sync.start();
    expect(h.imported.map((i) => i.annotation.id)).toEqual(['a1', 'a2']);
  });

  it('skips a row whose payload the backend could not parse', async () => {
    seed('p1', mark('a1'));
    server['p1']['a1'].payload = null;
    const h = harness();
    await h.sync.start();
    expect(h.imported).toEqual([]);
    // The row is still in the store, so the panel can list it.
    expect(annotationList('p1')).toHaveLength(1);
  });

  it('imports nothing, quietly, for a paper with no marks', async () => {
    const h = harness();
    await h.sync.start();
    expect(h.imported).toEqual([]);
    expect(api.putAnnotation).not.toHaveBeenCalled();
  });

  it('skips the import when the sidecar load failed — unknown is not empty', async () => {
    seed('p1', mark('a1'));
    listFails = 'network down';
    const h = harness();
    await h.sync.start();
    expect(h.imported).toEqual([]);
    // Marks drawn this session still save: the subscription outlives the bail.
    h.emit(created(mark('new1')));
    await h.sync.flush();
    expect(api.putAnnotation).toHaveBeenCalledWith('p1', 'new1', expect.anything());
  });
});

describe('ownership', () => {
  it('saves a mark the user draws', async () => {
    const h = harness();
    await h.sync.start();
    h.emit(created(mark('new1')));
    await h.sync.flush();
    expect(api.putAnnotation).toHaveBeenCalledWith(
      'p1',
      'new1',
      expect.objectContaining({ kind: 'highlight', quoted_text: 'quoted' }),
    );
  });

  it('never adopts a mark that came baked into the PDF', async () => {
    const h = harness();
    await h.sync.start();
    // A foreign annotation is in the document but was never created or
    // imported by us. Editing it must not copy it into the sidecar, or the
    // next load would draw it twice.
    h.objects.set('foreign', mark('foreign'));
    h.emit(updated(mark('foreign', { pageIndex: 9 })));
    await h.sync.flush();
    expect(api.putAnnotation).not.toHaveBeenCalled();
  });

  it('never deletes the sidecar row for a mark it does not own', async () => {
    const h = harness();
    await h.sync.start();
    h.objects.set('foreign', mark('foreign'));
    h.emit(deleted(mark('foreign')));
    await h.sync.flush();
    expect(api.deleteAnnotation).not.toHaveBeenCalled();
  });

  it('re-adopts a mark it imported, so an edit to it persists', async () => {
    seed('p1', mark('a1'));
    const h = harness();
    await h.sync.start();
    h.emit(updated(mark('a1', { color: colorHex('blue'), strokeColor: colorHex('blue') })));
    await h.sync.flush();
    expect(api.putAnnotation).toHaveBeenCalledWith(
      'p1',
      'a1',
      expect.objectContaining({ color: 'blue' }),
    );
  });

  it('refuses a subtype outside the whitelist even on a create', async () => {
    const h = harness();
    await h.sync.start();
    h.emit(created(mark('inky', { type: PdfAnnotationSubtype.INK })));
    await h.sync.flush();
    expect(api.putAnnotation).not.toHaveBeenCalled();
    // And having refused it, a later edit is still not ours.
    h.emit(updated(mark('inky', { type: PdfAnnotationSubtype.INK })));
    await h.sync.flush();
    expect(api.putAnnotation).not.toHaveBeenCalled();
  });

  it('ignores events belonging to another open document', async () => {
    const h = harness('p1', 'd1');
    await h.sync.start();
    h.emit(created(mark('other'), 'd2'));
    await h.sync.flush();
    expect(api.putAnnotation).not.toHaveBeenCalled();
  });

  it('ignores the loaded event entirely', async () => {
    const h = harness();
    await h.sync.start();
    h.emit({ type: 'loaded', documentId: 'd1', total: 12 });
    await h.sync.flush();
    expect(api.putAnnotation).not.toHaveBeenCalled();
  });
});

describe('debouncing', () => {
  it('writes a new mark straight away — a drawn mark should be durable fast', async () => {
    const h = harness();
    await h.sync.start();
    h.emit(created(mark('new1')));
    // No flush, no timer advance.
    expect(api.putAnnotation).toHaveBeenCalledTimes(1);
  });

  it('collapses a burst of edits into one write', async () => {
    vi.useFakeTimers();
    const h = harness();
    await h.sync.start();
    h.emit(created(mark('a1')));
    vi.mocked(api.putAnnotation).mockClear();
    for (const note of ['w', 'wo', 'wor', 'worth']) {
      h.emit(updated(mark('a1', { contents: note })));
    }
    expect(api.putAnnotation).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(20);
    expect(api.putAnnotation).toHaveBeenCalledTimes(1);
    expect(api.putAnnotation).toHaveBeenCalledWith(
      'p1',
      'a1',
      expect.objectContaining({ note: 'worth' }),
    );
    vi.useRealTimers();
  });

  it('skips a write when nothing actually changed', async () => {
    seed('p1', mark('a1'));
    const h = harness();
    await h.sync.start();
    // Selecting a mark can emit an update that changes nothing.
    h.emit(updated(mark('a1')));
    await h.sync.flush();
    expect(api.putAnnotation).not.toHaveBeenCalled();
  });

  it('notices a move, which only shows up in the payload', async () => {
    seed('p1', mark('a1'));
    const h = harness();
    await h.sync.start();
    const moved = mark('a1', {
      rect: { origin: { x: 99, y: 99 }, size: { width: 10, height: 10 } },
    });
    h.emit(updated(moved));
    await h.sync.flush();
    expect(api.putAnnotation).toHaveBeenCalledTimes(1);
  });

  it('drops a pending write when the mark is deleted first', async () => {
    vi.useFakeTimers();
    seed('p1', mark('a1'));
    const h = harness();
    await h.sync.start();
    h.emit(updated(mark('a1', { contents: 'half-typed' })));
    h.emit(deleted(mark('a1')));
    await vi.advanceTimersByTimeAsync(50);
    expect(api.deleteAnnotation).toHaveBeenCalledWith('p1', 'a1');
    expect(api.putAnnotation).not.toHaveBeenCalled();
    vi.useRealTimers();
  });
});

describe('deleting', () => {
  it('removes the row and forgets the mark', async () => {
    seed('p1', mark('a1'));
    const h = harness();
    await h.sync.start();
    h.emit(deleted(mark('a1')));
    await h.sync.flush();
    expect(api.deleteAnnotation).toHaveBeenCalledWith('p1', 'a1');
    expect(annotationList('p1')).toHaveLength(0);
    // Re-firing a delete for a mark we no longer own must not fire again.
    h.emit(deleted(mark('a1')));
    await h.sync.flush();
    expect(api.deleteAnnotation).toHaveBeenCalledTimes(1);
  });
});

describe('undo and redo', () => {
  it('restores the row on a redo that lands before the undo’s delete settles', async () => {
    const h = harness();
    await h.sync.start();
    h.emit(created(mark('a1')));
    await h.sync.flush();
    // Undo, then redo a moment later — the plugin emits delete then create for
    // the same id. The DELETE is still in flight when the create arrives, so a
    // save that read the cache eagerly would find the row it is about to
    // remove, call the mark unchanged, and skip the write.
    h.emit(deleted(mark('a1')));
    h.emit(created(mark('a1')));
    await h.sync.flush();
    expect(server['p1']['a1']).toBeDefined();
    expect(annotationList('p1')).toHaveLength(1);
  });

  it('leaves the row deleted when the undo is not redone', async () => {
    const h = harness();
    await h.sync.start();
    h.emit(created(mark('a1')));
    await h.sync.flush();
    h.emit(deleted(mark('a1')));
    await h.sync.flush();
    expect(server['p1']['a1']).toBeUndefined();
    expect(annotationList('p1')).toHaveLength(0);
  });
});

describe('closing the tab', () => {
  it('flushes a pending edit rather than losing it', async () => {
    vi.useFakeTimers();
    seed('p1', mark('a1'));
    const h = harness();
    await h.sync.start();
    h.emit(updated(mark('a1', { contents: 'just typed' })));
    await h.sync.destroy(); // before the debounce would have fired
    expect(api.putAnnotation).toHaveBeenCalledWith(
      'p1',
      'a1',
      expect.objectContaining({ note: 'just typed' }),
    );
    vi.useRealTimers();
  });

  it('stops listening', async () => {
    const h = harness();
    await h.sync.start();
    await h.sync.destroy();
    expect(h.unsubscribed).toBe(true);
    h.emit(created(mark('after')));
    await h.sync.flush();
    expect(api.putAnnotation).not.toHaveBeenCalled();
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
            resolve({
              paper_id: paperId,
              id,
              ...b,
              created_at: '2026-08-14T00:00:00Z',
              updated_at: '2026-08-14T01:00:00Z',
            });
        }),
    );
    h.emit(created(mark('a1')));
    const destroyed = h.sync.destroy();
    // The plugin can still deliver events during the flush await (teardown
    // churn, not a user edit). This must not arm a debounce timer that fires
    // a write after the document is closed.
    h.emit(updated(mark('a1', { contents: 'teardown noise' })));
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
    h.emit(created(mark('a1')));
    h.emit(created(mark('a2')));
    h.emit(created(mark('a3')));
    await h.sync.flush();
    expect(h.errors).toEqual(['saving the annotation failed: 500']);
  });

  it('reports again after a write succeeds in between', async () => {
    putFails = 'boom';
    const h = harness();
    await h.sync.start();
    h.emit(created(mark('a1')));
    await h.sync.flush();
    putFails = null;
    h.emit(created(mark('a2')));
    await h.sync.flush();
    putFails = 'boom again';
    h.emit(created(mark('a3')));
    await h.sync.flush();
    expect(h.errors).toEqual(['boom', 'boom again']);
  });

  it('does not throw out of the event handler', async () => {
    putFails = 'boom';
    const h = harness();
    await h.sync.start();
    expect(() => h.emit(created(mark('a1')))).not.toThrow();
    await expect(h.sync.flush()).resolves.toBeUndefined();
  });
});
