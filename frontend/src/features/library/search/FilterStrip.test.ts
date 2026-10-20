import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock(import('$features/library/api'), async (importOriginal) => {
  const mod = await importOriginal<typeof import('$features/library/api')>();
  return { ...mod, listPapers: vi.fn(async () => []) };
});

import FilterStrip from './FilterStrip.svelte';
import { projects, tags } from '$features/library/organization/state.svelte';
import { filters } from './state.svelte';

beforeEach(() => {
  vi.clearAllMocks();
  Object.assign(filters, {
    q: '', status: 'all', sort: 'year_desc', project: 'all', tag: undefined, starred: undefined,
  });
  projects.items = [{ id: 'pr1', name: 'NLP', paper_count: 3, created_at: '' }];
  tags.items = [{ id: 't1', name: 'nlp/eval', paper_count: 2, created_at: '' }];
});

describe('FilterStrip', () => {
  it('the status select writes the status qualifier', async () => {
    render(FilterStrip);
    await userEvent.selectOptions(screen.getByRole('combobox', { name: 'Filter by status' }), 'needs_review');
    expect(filters.status).toBe('needs_review');
    expect(filters.q).toBe('status:needs_review');
  });

  it('opens one pill zone at a time, folded by default', async () => {
    render(FilterStrip);
    const projectsBtn = screen.getByRole('button', { name: 'Projects' });
    const tagsBtn = screen.getByRole('button', { name: 'Star & tags' });
    expect(screen.queryByRole('group', { name: 'Project filters' })).not.toBeInTheDocument();
    await userEvent.click(projectsBtn);
    expect(projectsBtn).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByRole('button', { name: 'NLP 3' })).toBeInTheDocument();
    await userEvent.click(tagsBtn);
    expect(projectsBtn).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByRole('button', { name: 'NLP 3' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'nlp/eval 2' })).toBeInTheDocument();
    await userEvent.click(tagsBtn);
    expect(screen.queryByRole('group', { name: 'Star and tag filters' })).not.toBeInTheDocument();
  });

  it('badges a folded zone that holds an active filter', () => {
    filters.project = 'pr1';
    render(FilterStrip);
    expect(screen.getByRole('button', { name: 'Projects 1' })).toBeInTheDocument();
  });

  it('carries no sort select: the table headers sort', () => {
    render(FilterStrip);
    expect(screen.queryByRole('combobox', { name: 'Sort papers' })).not.toBeInTheDocument();
  });

  it('exports the filtered view for qualifier-only queries', () => {
    Object.assign(filters, { q: 'tag:nlp', tag: 'nlp' });
    render(FilterStrip);
    const link = screen.getByRole('link', { name: 'Export .bib' });
    expect(link.getAttribute('href')).toContain('/api/papers/export');
    expect(link.getAttribute('href')).toContain('tag=nlp');
    expect(link.getAttribute('href')).not.toContain('q=');
  });

  it('disables export while free-text search is active', () => {
    filters.q = 'fuzzing';
    render(FilterStrip);
    expect(screen.queryByRole('link', { name: 'Export .bib' })).not.toBeInTheDocument();
    expect(screen.getByTitle('Clear the search to export')).toHaveTextContent('Export .bib');
  });
});
