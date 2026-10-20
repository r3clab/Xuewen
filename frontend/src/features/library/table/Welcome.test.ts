import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import Welcome from './Welcome.svelte';
import { library } from '$features/library/state.svelte';
import { projects } from '$features/library/organization/state.svelte';
import { filters } from '$features/library/search/state.svelte';
import type { PaperSummary } from '$shared/api/generated/PaperSummary';

vi.mock(import('$features/library/api'), async (importOriginal) => {
  const mod = await importOriginal<typeof import('$features/library/api')>();
  return { ...mod, listPapers: vi.fn(async () => []) };
});

const paper: PaperSummary = {
  id: 'p1', title: 't', authors: [], venue: null, year: null, doi: null, arxiv_id: null,
  dblp_key: null, cite_key: null, url: null, source: null, status: 'resolved', added_at: '',
  name: null, starred: false, tags: [], projects: [],
};

beforeEach(() => {
  vi.clearAllMocks();
  library.papers = [];
  projects.items = [];
  Object.assign(filters, {
    q: '',
    status: 'all',
    sort: 'year_desc',
    project: 'all',
    tag: undefined,
    starred: undefined,
  });
});

describe('Welcome', () => {
  it('prompts to import when the library is empty', () => {
    render(Welcome);
    expect(screen.getByText(/library is empty/i)).toBeInTheDocument();
  });

  it('does not claim the library is empty when a filter is the cause', () => {
    filters.tag = 'os/rtos';
    render(Welcome);
    expect(screen.queryByText(/library is empty/i)).not.toBeInTheDocument();
    expect(screen.getByText(/No papers match/)).toBeInTheDocument();
  });

  it('tells you to click a paper to read once the library has items', () => {
    library.papers = [paper];
    render(Welcome);
    expect(screen.getByText(/click a paper to read/i)).toBeInTheDocument();
  });

  it('explains which filters matched nothing and offers Clear filters', async () => {
    library.papers = [];
    filters.q = 'quantum';
    render(Welcome);
    expect(screen.getByText(/No papers match/)).toBeInTheDocument();
    expect(screen.getByText(/“quantum”/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: /clear filters/i }));
    expect(filters.q).toBe('');
  });

  it('shows an add-papers tip when an empty project is selected', () => {
    library.papers = [];
    projects.items = [{ id: 'pr1', name: 'RTOS Fuzzing', paper_count: 0, created_at: '' }];
    filters.project = 'pr1';
    render(Welcome);
    expect(screen.getByText(/This project is empty/)).toBeInTheDocument();
  });
});
