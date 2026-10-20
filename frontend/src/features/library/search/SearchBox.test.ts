import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock(import('$features/library/api'), async (importOriginal) => {
  const mod = await importOriginal<typeof import('$features/library/api')>();
  return { ...mod, listPapers: vi.fn(async () => []) };
});
vi.mock(import('./api'), async (importOriginal) => {
  const mod = await importOriginal<typeof import('./api')>();
  return {
    ...mod,
    searchPapers: vi.fn(async () => ({ results: [], semantic: { available: true, reason: null } })),
  };
});

import SearchBox from './SearchBox.svelte';
import { filters, searchMeta, searchOpts } from './state.svelte';
import { viewer } from '$features/reader/tabs/state.svelte';

beforeEach(() => {
  filters.q = '';
  searchOpts.title = true;
  searchOpts.authors = true;
  searchOpts.abstract = true;
  searchOpts.body = true;
  searchOpts.keyword = true;
  searchOpts.semantic = true;
  searchMeta.semantic = { available: true, reason: null };
  searchMeta.pending = 0;
  viewer.tabs = [];
  viewer.activeId = null;
});

describe('SearchBox options popover', () => {
  it('opens with the options button and closes on Escape', async () => {
    render(SearchBox);
    await userEvent.click(screen.getByRole('button', { name: 'Search options' }));
    expect(screen.getByText('Engines')).toBeInTheDocument();
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByText('Engines')).not.toBeInTheDocument();
  });

  it('closes when clicking outside', async () => {
    render(SearchBox);
    await userEvent.click(screen.getByRole('button', { name: 'Search options' }));
    expect(screen.getByText('Engines')).toBeInTheDocument();
    await userEvent.click(document.body);
    expect(screen.queryByText('Engines')).not.toBeInTheDocument();
  });
});

describe('SearchBox narrowed hint', () => {
  it('is hidden when every available option is on', () => {
    render(SearchBox);
    expect(screen.queryByText('Narrowed')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Search options' })).toHaveAttribute('title', 'Search options');
  });

  it('does not count server-blocked semantic as narrowed', () => {
    searchMeta.semantic = { available: false, reason: 'semantic search not configured' };
    render(SearchBox);
    expect(screen.queryByText('Narrowed')).not.toBeInTheDocument();
  });

  it('shows on the options button when the user turns a field off', async () => {
    render(SearchBox);
    const options = screen.getByRole('button', { name: 'Search options' });
    await userEvent.click(options);
    await userEvent.click(screen.getByRole('button', { name: 'Body' }));
    expect(screen.getByText('Narrowed')).toBeInTheDocument();
    expect(options.title).toMatch(/narrowed/);
  });
});

describe('SearchBox over a PDF', () => {
  beforeEach(() => {
    viewer.tabs = [{ id: 'p1', title: 'A', name: null }];
    viewer.activeId = 'p1';
  });

  it('typing switches to the Library view, keeping the tab and the focus', async () => {
    render(SearchBox);
    const input = screen.getByRole('searchbox', { name: 'Search papers' });
    await userEvent.type(input, 'f');
    expect(viewer.activeId).toBe(null);
    expect(viewer.tabs).toHaveLength(1);
    expect(filters.q).toBe('f');
    expect(input).toHaveFocus();
  });

  it('clearing the box stays on the PDF', async () => {
    filters.q = 'f';
    render(SearchBox);
    await userEvent.clear(screen.getByRole('searchbox', { name: 'Search papers' }));
    expect(filters.q).toBe('');
    expect(viewer.activeId).toBe('p1');
  });
});
