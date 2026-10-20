import { beforeEach, describe, expect, it } from 'vitest';
import { filters, noMatchesBlame, relevanceSearchActive } from './state.svelte';

beforeEach(() => {
  Object.assign(filters, {
    q: '', status: 'all', project: 'all', tag: undefined, starred: undefined,
  });
});

describe('search state', () => {
  it('relevanceSearchActive is true only for free-text or author terms', () => {
    for (const [q, want] of [
      ['', false],
      ['tag:nlp project:X is:starred', false],
      ['fuzz', true],
      ['author:smith', true],
    ] as const) {
      filters.q = q;
      expect(relevanceSearchActive(), q).toBe(want);
    }
  });

  it('noMatchesBlame names every active filter with the · join', () => {
    Object.assign(filters, { q: 'fuzz', tag: 'nlp/eval', starred: true });
    expect(noMatchesBlame([])).toBe('No papers match “fuzz” · nlp/eval · starred.');
  });
});
