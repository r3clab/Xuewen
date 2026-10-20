import { render, screen, waitFor, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock(import('$features/library/api'), async (importOriginal) => {
  const mod = await importOriginal<typeof import('$features/library/api')>();
  return {
    ...mod,
    listPapers: vi.fn(async () => corpus),
    getPreviewMeta: vi.fn(async () => ({ pages: 2, page_width: 640, page_height: 828 })),
  };
});

import * as api from '$features/library/api';
import FilePicker from './FilePicker.svelte';
import { library } from '$features/library/state.svelte';
import { finder } from './state.svelte';
import { viewer } from '$features/reader/tabs/state.svelte';
import type { PaperSummary } from '$shared/api/generated/PaperSummary';

function paper(over: Partial<PaperSummary> & { id: string }): PaperSummary {
  return {
    title: null, authors: [], venue: null, year: null, doi: null, arxiv_id: null,
    dblp_key: null, cite_key: null, url: null, source: null, status: 'resolved',
    added_at: '', name: null, starred: false, tags: [], projects: [], ...over,
  };
}

let corpus: PaperSummary[] = [];

beforeEach(() => {
  vi.clearAllMocks();
  corpus = [
    paper({ id: 'p1', name: 'transformer', title: 'Attention Is All You Need', authors: ['Vaswani'], cite_key: 'vaswani2017' }),
    paper({ id: 'p2', title: 'Deep Residual Learning', authors: ['He'] }),
    paper({ id: 'p3', name: 'swe-bench', title: 'SWE-bench', status: 'needs_review' }),
  ];
  library.papers = [];
  viewer.tabs = [];
  viewer.activeId = null;
  finder.open = true;
});

describe('FilePicker corpus', () => {
  it('searches the whole library, not the filtered view', async () => {
    library.papers = [corpus[0]];
    render(FilePicker);
    await waitFor(() => expect(screen.getAllByRole('option')).toHaveLength(3));
    expect(api.listPapers).toHaveBeenCalledWith({ q: '', status: 'all', sort: 'added_desc', project: 'all' });
  });

  it('falls back to the loaded list until the fetch resolves', () => {
    library.papers = [corpus[1]];
    render(FilePicker);
    expect(screen.getAllByRole('option')).toHaveLength(1);
  });
});

describe('FilePicker matching', () => {
  it('matches a name', async () => {
    render(FilePicker);
    await waitFor(() => expect(screen.getAllByRole('option')).toHaveLength(3));
    await userEvent.type(screen.getByRole('combobox'), 'swe');
    const options = screen.getAllByRole('option');
    expect(options).toHaveLength(1);
    expect(within(options[0]).getByText('swe-bench', { selector: 'span' })).toBeInTheDocument();
  });

  it('matches a title', async () => {
    render(FilePicker);
    await waitFor(() => expect(screen.getAllByRole('option')).toHaveLength(3));
    await userEvent.type(screen.getByRole('combobox'), 'residual');
    expect(screen.getAllByRole('option')).toHaveLength(1);
  });

  it('matches neither authors nor cite keys', async () => {
    render(FilePicker);
    await waitFor(() => expect(screen.getAllByRole('option')).toHaveLength(3));
    await userEvent.type(screen.getByRole('combobox'), 'vaswani');
    expect(screen.queryAllByRole('option')).toHaveLength(0);
    expect(screen.getByText(/Nothing matches/)).toBeInTheDocument();
  });
});

describe('FilePicker keyboard', () => {
  it('opens the highlighted paper as a tab and closes', async () => {
    render(FilePicker);
    await waitFor(() => expect(screen.getAllByRole('option')).toHaveLength(3));
    await userEvent.keyboard('{ArrowDown}{Enter}');
    expect(viewer.activeId).toBe('p2');
    expect(finder.open).toBe(false);
  });

  it('stops at the ends of the list', async () => {
    render(FilePicker);
    await waitFor(() => expect(screen.getAllByRole('option')).toHaveLength(3));
    await userEvent.keyboard('{ArrowUp}{ArrowUp}');
    expect(screen.getAllByRole('option')[0]).toHaveAttribute('aria-selected', 'true');
    await userEvent.keyboard('{ArrowDown}{ArrowDown}{ArrowDown}{ArrowDown}');
    expect(screen.getAllByRole('option')[2]).toHaveAttribute('aria-selected', 'true');
  });

  it('keeps the highlighted row when the full corpus arrives late', async () => {
    let release: (papers: PaperSummary[]) => void = () => {};
    vi.mocked(api.listPapers).mockReturnValueOnce(
      new Promise<PaperSummary[]>((resolve) => (release = resolve)),
    );
    library.papers = corpus.slice(0, 2);
    render(FilePicker);
    await userEvent.keyboard('{ArrowDown}');
    release(corpus);
    await waitFor(() => expect(screen.getAllByRole('option')).toHaveLength(3));
    expect(screen.getAllByRole('option')[1]).toHaveAttribute('aria-selected', 'true');
    await userEvent.keyboard('{Enter}');
    expect(viewer.activeId).toBe('p2');
  });

  it('closes on Escape without opening anything', async () => {
    render(FilePicker);
    await waitFor(() => expect(screen.getAllByRole('option')).toHaveLength(3));
    await userEvent.keyboard('{Escape}');
    expect(finder.open).toBe(false);
    expect(viewer.tabs).toHaveLength(0);
  });
});

describe('FilePicker preview', () => {
  it('renders one image per page for the highlighted row', async () => {
    render(FilePicker);
    await waitFor(() => expect(screen.getAllByRole('option')).toHaveLength(3));
    await waitFor(() => expect(screen.getAllByRole('img')).toHaveLength(2));
    expect(api.getPreviewMeta).toHaveBeenCalledWith('p1');
    expect(screen.getByAltText('Page 1')).toHaveAttribute('src', '/papers/p1/preview/0');
  });

  it('shows the fallback card when the render fails', async () => {
    vi.mocked(api.getPreviewMeta).mockRejectedValueOnce(new Error('unprocessable'));
    render(FilePicker);
    await waitFor(() => expect(screen.getByText(/could not be rendered/)).toBeInTheDocument());
    expect(screen.queryAllByRole('img')).toHaveLength(0);
  });

  it('paints only the last row arrowed to', async () => {
    render(FilePicker);
    await waitFor(() => expect(screen.getAllByRole('option')).toHaveLength(3));
    await userEvent.keyboard('{ArrowDown}{ArrowDown}');
    await waitFor(() => expect(screen.getByAltText('Page 1')).toHaveAttribute('src', '/papers/p3/preview/0'));
    expect(vi.mocked(api.getPreviewMeta).mock.calls.map(([id]) => id)).not.toContain('p2');
  });
});
