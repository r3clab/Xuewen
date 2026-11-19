import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock(import('$features/library/api'), async (importOriginal) => {
  const mod = await importOriginal<typeof import('$features/library/api')>();
  return {
    ...mod,
    listPapers: vi.fn(async () => []),
    exportPapers: vi.fn(async () => new Blob(['@article{a,}'])),
  };
});
vi.mock(import('$shared/browser/download'), () => ({ downloadBlob: vi.fn() }));

import FilterStrip from './FilterStrip.svelte';
import * as api from '$features/library/api';
import { downloadBlob } from '$shared/browser/download';
import { projects, tags } from '$features/library/organization/state.svelte';
import { bibFormat, library } from '$features/library/state.svelte';
import { filters } from './state.svelte';
import type { PaperSummary } from '$shared/api/generated/PaperSummary';

beforeEach(() => {
  vi.clearAllMocks();
  filters.q = '';
  projects.items = [{ id: 'pr1', name: 'NLP', paper_count: 3, created_at: '' }];
  tags.items = [{ id: 't1', name: 'nlp/eval', paper_count: 2, created_at: '' }];
});

describe('FilterStrip', () => {
  it('the status select writes the status qualifier, and shows the one the query holds', async () => {
    render(FilterStrip);
    const select = screen.getByRole<HTMLSelectElement>('combobox', { name: 'Filter by status' });
    await userEvent.selectOptions(select, 'Needs review');
    expect(filters.q).toBe('status:needs_review');
    expect(filters.status).toBe('needs_review');
    await userEvent.selectOptions(select, 'All status');
    expect(filters.q).toBe('');
  });

  it('follows a status typed into the search box', async () => {
    render(FilterStrip);
    filters.q = 'status:resolved';
    await vi.waitFor(() =>
      expect(screen.getByRole<HTMLSelectElement>('combobox', { name: 'Filter by status' }).value).toBe('Resolved'),
    );
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
    filters.q = 'project:nlp';
    render(FilterStrip);
    expect(screen.getByRole('button', { name: 'Projects 1' })).toBeInTheDocument();
  });

  it('carries no sort select: the table headers sort', () => {
    render(FilterStrip);
    expect(screen.queryByRole('combobox', { name: 'Sort papers' })).not.toBeInTheDocument();
  });

  it('exports exactly the rows shown, ranked search included', async () => {
    filters.q = 'fuzzing';
    bibFormat.value = 'biblatex';
    library.papers = [{ id: 'b' }, { id: 'a' }] as PaperSummary[];
    render(FilterStrip);
    await userEvent.click(screen.getByRole('button', { name: 'Export .bib' }));
    expect(api.exportPapers).toHaveBeenCalledWith({ ids: ['b', 'a'], format: 'biblatex' });
    await vi.waitFor(() => expect(downloadBlob).toHaveBeenCalledWith(expect.any(Blob), 'xuewen.bib'));
  });
});
