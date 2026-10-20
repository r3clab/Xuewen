import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock(import('$features/library/api'), async (importOriginal) => {
  const mod = await importOriginal<typeof import('$features/library/api')>();
  return { ...mod, listPapers: vi.fn(async () => []) };
});
vi.mock(import('$features/library/search/api'), async (importOriginal) => {
  const mod = await importOriginal<typeof import('$features/library/search/api')>();
  return {
    ...mod,
    searchPapers: vi.fn(async () => ({ results: [], semantic: { available: true, reason: null } })),
  };
});

import * as api from '$features/library/api';
import TopBar from './TopBar.svelte';
import { stats } from '$features/library/state.svelte';
import { filters } from '$features/library/search/state.svelte';
import { viewer } from '$features/reader/tabs/state.svelte';

beforeEach(() => {
  vi.clearAllMocks();
  filters.q = '';
  filters.status = 'all';
  stats.value = { total: 9, resolved: 8, needs_review: 1 };
  viewer.tabs = [];
  viewer.activeId = null;
});

describe('TopBar search', () => {
  it('carries the search box and no list-pane toggle', () => {
    render(TopBar);
    expect(screen.getByRole('searchbox', { name: 'Search papers' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /list pane/i })).not.toBeInTheDocument();
  });
});

describe('TopBar review count', () => {
  it('clicking the review count filters the list to needs-review', async () => {
    render(TopBar);
    await userEvent.click(screen.getByRole('button', { name: /1 to review/ }));
    expect(filters.status).toBe('needs_review');
    expect(api.listPapers).toHaveBeenCalled();
  });

  it('clicking the review count while reading returns to the Library', async () => {
    viewer.tabs = [{ id: 'p1', title: 'A', name: null }];
    viewer.activeId = 'p1';
    render(TopBar);
    await userEvent.click(screen.getByRole('button', { name: /1 to review/ }));
    expect(viewer.activeId).toBe(null);
    expect(filters.status).toBe('needs_review');
  });

  it('hides the review count when nothing needs review', () => {
    stats.value = { total: 9, resolved: 9, needs_review: 0 };
    render(TopBar);
    expect(screen.queryByText(/to review/)).not.toBeInTheDocument();
  });
});

describe('TopBar counts', () => {
  it('shows the match count instead of library total while searching', async () => {
    const { library } = await import('$features/library/state.svelte');
    filters.q = 'fuzzing';
    library.papers = [
      { id: 'p1', title: 'A', authors: [], venue: null, year: null, doi: null,
        arxiv_id: null, dblp_key: null, cite_key: null, url: null, source: null,
        status: 'resolved', added_at: '2026-01-01', name: null, starred: false, tags: [], projects: [] },
    ];
    render(TopBar);
    expect(screen.getByText('1 match')).toBeInTheDocument();
    expect(screen.queryByText('9 papers')).not.toBeInTheDocument();
  });
});

describe('TopBar theme toggle', () => {
  it('theme button names current and next mode', async () => {
    const { theme } = await import('$shared/ui/theme.svelte');
    theme.mode = 'light';
    render(TopBar);
    expect(screen.getByRole('button', { name: /theme: light/i }).title)
      .toMatch(/click for dark/i);
  });
});
