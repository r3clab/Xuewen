import { beforeEach, describe, expect, it, vi, type Mock } from 'vitest';

vi.mock(import('./api'), async (importOriginal) => {
  const mod = await importOriginal<typeof import('./api')>();
  return { ...mod, setStar: vi.fn(async () => {}), listPapers: vi.fn(async () => []) };
});
vi.mock(import('$features/library/search/api'), async (importOriginal) => {
  const mod = await importOriginal<typeof import('$features/library/search/api')>();
  return {
    ...mod,
    searchPapers: vi.fn(async () => ({
      semantic: { available: true, reason: null },
      results: [
        {
          paper: { id: 'p1', title: 'T', authors: [], venue: null, year: null, doi: null,
                   arxiv_id: null, dblp_key: null, cite_key: null, url: null, source: null,
                   status: 'resolved' as const, added_at: '', name: null, starred: false, tags: [], projects: [] },
          match: { engine: 'keyword' as const, field: 'body', snippet: 'a <mark>hit</mark>', page: 7 },
        },
      ],
    })),
  };
});

import * as api from './api';
import * as searchApi from '$features/library/search/api';
import { library } from './state.svelte';
import { loadPapers, toggleStar } from './actions';
import { filters, searchMeta, searchOpts } from '$features/library/search/state.svelte';
import { toasts } from '$shared/ui/toasts.svelte';
import type { PaperSummary } from '$shared/api/generated/PaperSummary';

function paper(id: string): PaperSummary {
  return {
    id, title: 'T', authors: [], venue: null, year: null, doi: null, arxiv_id: null,
    dblp_key: null, cite_key: null, url: null, source: null, status: 'resolved',
    added_at: '', name: null, starred: false, tags: [], projects: [],
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  library.papers = [paper('p1')];
  filters.starred = undefined;
  toasts.items = [];
});

describe('optimistic star toggle', () => {
  it('flips the star before the server responds', async () => {
    let resolve!: () => void;
    (api.setStar as Mock).mockImplementation(() => new Promise<void>((r) => (resolve = r)));
    const done = toggleStar('p1');
    expect(library.papers[0].starred).toBe(true); // flipped before the await settles
    resolve();
    await done;
    expect(library.papers[0].starred).toBe(true);
  });

  it('rolls back and toasts when the server rejects', async () => {
    (api.setStar as Mock).mockRejectedValue(new Error('boom'));
    await toggleStar('p1');
    expect(library.papers[0].starred).toBe(false);
    expect(toasts.items.some((t) => t.kind === 'error')).toBe(true);
  });
});

describe('loadPapers', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    Object.assign(filters, {
      q: '', status: 'all', project: 'all', tag: undefined, starred: undefined,
    });
    Object.assign(searchOpts, {
      title: true, authors: true, abstract: true, body: true, keyword: true, semantic: true,
    });
    searchMeta.byId = {};
    searchMeta.semantic = { available: true, reason: null };
    searchMeta.pending = 0;
  });

  it('loadPapers uses searchPapers when q is set and stores match info', async () => {
    filters.q = 'fuzz';
    await loadPapers();
    expect(searchApi.searchPapers).toHaveBeenCalledOnce();
    expect(library.papers.map((p) => p.id)).toEqual(['p1']);
    expect(searchMeta.byId['p1'].snippet).toContain('<mark>');
  });

  it('loadPapers uses listPapers and clears match info when q is empty', async () => {
    searchMeta.byId = { p1: { engine: 'keyword', field: 'body', snippet: 'x', page: null } };
    await loadPapers();
    expect(api.listPapers).toHaveBeenCalledOnce();
    expect(Object.keys(searchMeta.byId)).toHaveLength(0);
  });

  it('loadPapers marks the library loaded once a load settles, even a failed one', async () => {
    library.loaded = false;
    (api.listPapers as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new Error('down'));
    await loadPapers();
    expect(library.error).toBe('down');
    expect(library.loaded).toBe(true);
  });
});
