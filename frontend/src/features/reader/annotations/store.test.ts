import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Annotation } from '$shared/api/generated/Annotation';
import type { NewAnnotation } from '$shared/api/generated/NewAnnotation';

/// The server's rows, keyed by paper. The fakes below read and write this so
/// the tests exercise the cache-vs-server relationship, not a stub that always
/// echoes its input.
const server: Record<string, Record<string, Annotation>> = {};
let listFails: string | null = null;

vi.mock(import('./api'), () => ({
  listAnnotations: vi.fn(async (paperId: string) => {
    if (listFails) throw new Error(listFails);
    return Object.values(server[paperId] ?? {});
  }),
  putAnnotation: vi.fn(async (paperId: string, id: string, body: NewAnnotation) => {
    const prev = server[paperId]?.[id];
    const saved: Annotation = {
      paper_id: paperId,
      id,
      ...body,
      created_at: prev?.created_at ?? '2026-08-14T00:00:00Z',
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
import {
  annotationList,
  annotations,
  dropAnnotations,
  isLoaded,
  loadAnnotations,
  removeAnnotation,
  saveAnnotation,
} from './store.svelte';

function body(over: Partial<NewAnnotation> = {}): NewAnnotation {
  return {
    page_index: 0,
    kind: 'highlight',
    color: 'amber',
    quoted_text: 'quoted',
    note: null,
    payload: { annotation: { id: 'x' } },
    ...over,
  };
}

function seed(paperId: string, id: string, over: Partial<Annotation> = {}): void {
  (server[paperId] ??= {})[id] = {
    paper_id: paperId,
    id,
    ...body(),
    created_at: '2026-08-14T00:00:00Z',
    updated_at: '2026-08-14T00:00:00Z',
    ...over,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  for (const k of Object.keys(server)) delete server[k];
  listFails = null;
  annotations.byPaper = {};
  annotations.loaded = {};
  annotations.error = {};
});

describe('loading', () => {
  it('fills the cache and marks the paper loaded', async () => {
    seed('p1', 'a1');
    await loadAnnotations('p1');
    expect(isLoaded('p1')).toBe(true);
    expect(annotationList('p1')).toHaveLength(1);
    expect(annotations.error['p1']).toBeNull();
  });

  it('records a failure instead of throwing, and stays unloaded', async () => {
    listFails = 'network down';
    await expect(loadAnnotations('p1')).resolves.toBeUndefined();
    expect(annotations.error['p1']).toBe('network down');
    // Unloaded is NOT the same as empty: the sync loop must not read this
    // paper as "no marks" and start saving over the server's copy.
    expect(isLoaded('p1')).toBe(false);
  });

  it('replaces rather than merges, so a server-side delete disappears', async () => {
    seed('p1', 'a1');
    seed('p1', 'a2');
    await loadAnnotations('p1');
    delete server['p1']['a1'];
    await loadAnnotations('p1');
    expect(annotationList('p1').map((a) => a.id)).toEqual(['a2']);
  });
});

describe('ordering', () => {
  it('sorts by page, then creation time, then id', async () => {
    seed('p1', 'later-page', { page_index: 5 });
    seed('p1', 'b', { page_index: 1, created_at: '2026-08-14T00:00:02Z' });
    seed('p1', 'a', { page_index: 1, created_at: '2026-08-14T00:00:01Z' });
    seed('p1', 'tie-z', { page_index: 1, created_at: '2026-08-14T00:00:01Z' });
    await loadAnnotations('p1');
    expect(annotationList('p1').map((x) => x.id)).toEqual(['a', 'tie-z', 'b', 'later-page']);
  });

  it('is empty for a paper nobody has loaded', () => {
    expect(annotationList('nope')).toEqual([]);
  });
});

describe('writing', () => {
  it('caches the server echo, not the local guess', async () => {
    await saveAnnotation('p1', 'a1', body());
    const row = annotations.byPaper['p1']['a1'];
    expect(row.updated_at).toBe('2026-08-14T01:00:00Z');
    expect(row.paper_id).toBe('p1');
  });

  it('re-saving the same id replaces rather than duplicating', async () => {
    await saveAnnotation('p1', 'a1', body());
    await saveAnnotation('p1', 'a1', body({ color: 'blue' }));
    expect(annotationList('p1')).toHaveLength(1);
    expect(annotationList('p1')[0].color).toBe('blue');
  });
});

describe('removing', () => {
  it('drops one row from both the server and the cache', async () => {
    seed('p1', 'a1');
    seed('p1', 'a2');
    await loadAnnotations('p1');
    await removeAnnotation('p1', 'a1');
    expect(annotationList('p1').map((a) => a.id)).toEqual(['a2']);
    expect(Object.keys(server['p1'])).toEqual(['a2']);
  });

  it('scopes every operation to its own paper', async () => {
    seed('p1', 'a1');
    seed('p2', 'a1'); // same annotation id, different paper
    await loadAnnotations('p1');
    await loadAnnotations('p2');
    await removeAnnotation('p1', 'a1');
    expect(annotationList('p1')).toHaveLength(0);
    expect(annotationList('p2')).toHaveLength(1);
    expect(Object.keys(server['p2'])).toEqual(['a1']);
  });

  it('dropping a closed tab forgets the cache but not the server rows', async () => {
    seed('p1', 'a1');
    await loadAnnotations('p1');
    dropAnnotations('p1');
    expect(isLoaded('p1')).toBe(false);
    expect(annotationList('p1')).toHaveLength(0);
    expect(api.deleteAnnotation).not.toHaveBeenCalled();
    expect(Object.keys(server['p1'])).toEqual(['a1']);
  });
});
