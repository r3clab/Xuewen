import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock(import('$features/library/api'), async (importOriginal) => {
  const mod = await importOriginal<typeof import('$features/library/api')>();
  return { ...mod, listPapers: vi.fn(async () => []) };
});
vi.mock(import('./api'), async (importOriginal) => {
  const mod = await importOriginal<typeof import('./api')>();
  return {
    ...mod,
    searchPapers: vi.fn(async () => ({
      semantic: { available: true, reason: null },
      results: [
        {
          paper: { id: 'p1', title: 'T', authors: [], venue: null, year: null, doi: null,
                   arxiv_id: null, dblp_key: null, cite_key: null, url: null, source: null,
                   status: 'resolved' as const, added_at: '', name: null, starred: false, tags: [], projects: [] },
          match: {
            engine: 'keyword' as const,
            field: 'body' as const,
            snippet: [{ text: 'a ', marked: false }, { text: 'hit', marked: true }],
            page: 7,
          },
        },
      ],
    })),
    getSearchStatus: vi.fn(async () => ({
      fts: { indexed: 1, pending: 0, failed: 0 },
      vectors: { indexed: 0, pending: 3, failed: 0 },
      semantic_available: false,
      reason: 'no key',
    })),
  };
});

import * as searchApi from './api';
import { engines, filters, searchMeta, semanticBlocked } from './state.svelte';
import {
  loadSearchStatus,
  setProjectFilter,
  setSearch,
  setStarFilter,
  setTagFilter,
  toggleSearchEngine,
  toggleSearchField,
} from './actions';

beforeEach(() => {
  vi.clearAllMocks();
  filters.q = '';
  Object.assign(engines, { keyword: true, semantic: true });
  searchMeta.byId = {};
  searchMeta.semantic = { available: true, reason: null };
  searchMeta.pending = 0;
});

describe('search actions', () => {
  it('setSearch debounces: keyword-only first, then full', async () => {
    vi.useFakeTimers();
    setSearch('fuzz');
    expect(searchApi.searchPapers).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(200);
    expect(searchApi.searchPapers).toHaveBeenCalledTimes(1);
    expect(vi.mocked(searchApi.searchPapers).mock.calls[0][2]).toBe(true); // keywordOnly
    await vi.advanceTimersByTimeAsync(600);
    expect(searchApi.searchPapers).toHaveBeenCalledTimes(2);
    expect(vi.mocked(searchApi.searchPapers).mock.calls[1][2]).toBe(false);
    vi.useRealTimers();
  });

  it('pill setters edit the query string and combine (no mutual exclusion)', () => {
    void setTagFilter('nlp');
    void setStarFilter(true);
    void setProjectFilter('My Thesis');
    expect(filters.q).toContain('tag:nlp');
    expect(filters.q).toContain('is:starred');
    expect(filters.q).toContain('project:"My Thesis"');
    void setTagFilter(null);
    expect(filters.q).not.toContain('tag:nlp');
    expect(filters.q).toContain('is:starred');
  });

  it('toggleSearchField writes in: tokens into the query', () => {
    vi.useFakeTimers();
    setSearch('attention');
    toggleSearchField('body'); // turn body off
    expect(filters.q).toContain('in:title');
    expect(filters.q).toContain('in:abstract');
    expect(filters.q).toContain('in:authors');
    expect(filters.q).toContain('in:notes');
    expect(filters.q).not.toContain('in:body');
    expect(filters.fields).not.toContain('body');
    vi.useRealTimers();
  });

  it('cannot turn off the last field or engine', () => {
    filters.q = 'in:body attention';
    toggleSearchField('body'); // body is the last field
    expect(filters.fields).toEqual(['body']);
    engines.semantic = false;
    toggleSearchEngine('keyword'); // keyword is the last engine
    expect(engines.keyword).toBe(true);
  });

  it('semanticBlocked when nothing embedded is selected, or unavailable backend', async () => {
    expect(semanticBlocked()).toBe(false);
    // authors + notes are keyword-only — neither is chunked or embedded.
    filters.q = 'in:authors in:notes x';
    expect(semanticBlocked()).toBe(true);
    filters.q = 'x';
    await loadSearchStatus();
    expect(searchMeta.semantic.available).toBe(false);
    expect(semanticBlocked()).toBe(true);
    expect(searchMeta.pending).toBe(3); // max(fts.pending, vectors.pending)
  });
});
