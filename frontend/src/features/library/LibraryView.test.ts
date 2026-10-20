import { render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock(import('./api'), async (importOriginal) => {
  const mod = await importOriginal<typeof import('./api')>();
  return {
    ...mod,
    listPapers: vi.fn(async () => []),
    getStats: vi.fn(async () => ({ total: 0, resolved: 0, needs_review: 0 })),
  };
});
vi.mock(import('$features/library/organization/api'), async (importOriginal) => {
  const mod = await importOriginal<typeof import('$features/library/organization/api')>();
  return {
    ...mod,
    listProjects: vi.fn(async () => []),
    listTags: vi.fn(async () => []),
  };
});

import LibraryView from './LibraryView.svelte';
import { library } from './state.svelte';
import { projects, tags } from '$features/library/organization/state.svelte';
import { filters, searchMeta } from '$features/library/search/state.svelte';
import type { PaperSummary } from '$shared/api/generated/PaperSummary';

const paper: PaperSummary = {
  id: 'p1', title: 'First Paper', authors: [], venue: null, year: null, doi: null,
  arxiv_id: null, dblp_key: null, cite_key: null, url: null, source: null,
  status: 'resolved', added_at: '', name: null, starred: false, tags: [], projects: [],
};

beforeEach(() => {
  vi.clearAllMocks();
  Object.assign(library, { papers: [], loading: false, loaded: true, error: null });
  Object.assign(filters, {
    q: '', status: 'all', sort: 'year_desc', project: 'all', tag: undefined, starred: undefined,
  });
  searchMeta.byId = {};
  projects.items = [];
  tags.items = [];
});

describe('LibraryView', () => {
  it('shows a spinner, not "library is empty", until the first load settles', () => {
    Object.assign(library, { loading: true, loaded: false });
    render(LibraryView);
    expect(screen.getByRole('status')).toHaveTextContent('Loading library…');
    expect(screen.queryByText(/library is empty/i)).not.toBeInTheDocument();
    expect(screen.getByRole('group', { name: 'Library filters' })).toBeInTheDocument();
  });

  it('keeps the table up during a later reload', () => {
    Object.assign(library, { papers: [paper], loading: true });
    render(LibraryView);
    expect(screen.getByRole('table')).toBeInTheDocument();
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
  });

  it('shows the empty-library hero when nothing is filtered', () => {
    render(LibraryView);
    expect(screen.getByText(/library is empty/i)).toBeInTheDocument();
  });

  it('keeps the filter strip visible when a filter matches nothing', () => {
    Object.assign(filters, { q: 'tag:x', tag: 'x' });
    render(LibraryView);
    expect(screen.getByText(/No papers match/)).toBeInTheDocument();
    expect(screen.getByRole('group', { name: 'Library filters' })).toBeInTheDocument();
  });

  it('surfaces a load error above a stale table', () => {
    Object.assign(library, { papers: [paper], error: 'boom' });
    render(LibraryView);
    expect(screen.getByRole('alert')).toHaveTextContent('boom');
    expect(screen.getByRole('table')).toBeInTheDocument();
  });

  it('a load error with nothing to show never claims the library is empty', () => {
    library.error = 'boom';
    render(LibraryView);
    expect(screen.getByRole('alert')).toHaveTextContent('boom');
    expect(screen.queryByText(/library is empty/i)).not.toBeInTheDocument();
  });
});
