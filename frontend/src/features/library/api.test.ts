import { describe, expect, it } from 'vitest';
import { exportUrl } from './api';
import type { Filters } from '$features/library/search/types';

const baseFilters: Filters = { q: '', status: 'all', sort: 'year_desc', project: 'all' };

describe('exportUrl', () => {
  it('builds a url with sort and format when no other filters are set', () => {
    const url = exportUrl(baseFilters, 'bibtex');
    expect(url).toBe('/api/papers/export?sort=year_desc&format=bibtex');
  });

  it('includes active search, status, and project filters', () => {
    const url = exportUrl(
      { q: 'graph', status: 'resolved', sort: 'year_desc', project: 'p1' },
      'biblatex',
    );
    expect(url).toContain('q=graph');
    expect(url).toContain('status=resolved');
    expect(url).toContain('project=p1');
    expect(url).toContain('format=biblatex');
  });

  it('strips a qualifier-only q and rides on the parsed filters instead', () => {
    // A tag pill writes 'tag:nlp' into filters.q and the cached parse sets
    // filters.tag; the endpoint would LIKE-match the literal 'tag:nlp' as q,
    // so the export must travel as tag=, with q dropped.
    const url = exportUrl(
      { q: 'tag:nlp', status: 'all', sort: 'year_desc', project: 'all', tag: 'nlp' },
      'bibtex',
    );
    expect(url).toContain('tag=nlp');
    expect(url).not.toContain('q=');
  });
});
