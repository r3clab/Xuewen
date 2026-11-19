import { beforeEach, describe, expect, it } from 'vitest';
import { projects } from '$features/library/organization/state.svelte';
import { anyFilterActive, filters, noMatchesBlame } from './state.svelte';

beforeEach(() => {
  filters.q = '';
  projects.items = [];
});

describe('the library query', () => {
  it('searches only for free-text or author terms', () => {
    for (const [q, want] of [
      ['', false],
      ['tag:nlp project:X is:starred', false],
      ['fuzz', true],
      ['author:smith', true],
    ] as const) {
      filters.q = q;
      expect(filters.searching, q).toBe(want);
    }
  });

  it('reads every filter off the one query string', () => {
    filters.q = 'tag:nlp/eval is:starred status:needs_review in:title fuzz';
    expect(filters.tag).toBe('nlp/eval');
    expect(filters.starred).toBe(true);
    expect(filters.status).toBe('needs_review');
    expect(filters.fields).toEqual(['title']);
    expect(anyFilterActive()).toBe(true);
    filters.q = '  ';
    expect(anyFilterActive()).toBe(false);
  });

  it('resolves project: to the project it names, case-insensitively', () => {
    projects.items = [{ id: 'pr1', name: 'Thesis', paper_count: 2, created_at: '' }];
    filters.q = 'project:thesis';
    expect(filters.project?.id).toBe('pr1');
    filters.q = 'project:nosuch';
    expect(filters.project).toBeNull();
  });

  it('noMatchesBlame names every active filter with the · join', () => {
    filters.q = 'fuzz tag:nlp/eval is:starred';
    expect(noMatchesBlame()).toBe('No papers match “fuzz” · nlp/eval · starred.');
  });
});
