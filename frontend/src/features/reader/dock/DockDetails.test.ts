import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { chat } from '$features/reader/ask/chat.svelte';
import { identifyState } from '$features/identify/state.svelte';
import { library } from '$features/library/state.svelte';
import { removePaper } from '$app/actions';
import { openTab, viewer } from '$features/reader/tabs/state.svelte';
import { appSettings } from '$shared/api/settings.svelte';
import type { PaperSummary } from '$shared/api/generated/PaperSummary';
import DockDetails from './DockDetails.svelte';

function paper(id: string): PaperSummary {
  return {
    id, title: id, authors: [], venue: null, year: null, doi: null, arxiv_id: null,
    dblp_key: null, cite_key: null, url: null, source: null, status: 'resolved',
    added_at: '', name: null, starred: false, tags: [], projects: [],
  };
}

describe('removePaper', () => {
  beforeEach(() => {
    library.papers = [];
    viewer.tabs = [];
    viewer.activeId = null;
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        new Response(JSON.stringify({ total: 0, resolved: 0, needs_review: 0 }), {
          status: 200,
          headers: { 'content-type': 'application/json' },
        }),
      ),
    );
  });

  it('deletes on the server, closes the tab, and drops it from the list', async () => {
    library.papers = [paper('x'), paper('y')];
    openTab(paper('x'));
    expect(viewer.tabs.length).toBe(1);

    await removePaper('x');

    expect(library.papers.map((p) => p.id)).toEqual(['y']);
    expect(viewer.tabs.length).toBe(0);
    expect(viewer.activeId).toBe(null);
    expect((globalThis.fetch as ReturnType<typeof vi.fn>).mock.calls[0][1]).toMatchObject({
      method: 'DELETE',
    });
  });
});

function detail(id: string) {
  return {
    id, title: 'Attention', authors: ['Vaswani'], venue: 'NeurIPS', year: 2017,
    doi: '10.1/x', arxiv_id: '1706.03762', dblp_key: null, cite_key: 'vaswani2017',
    url: null, source: 'crossref', status: 'resolved', added_at: '2026-07-08T00:00:00Z',
    abstract: 'The dominant sequence transduction models…',
    name: null, starred: false, tags: [], projects: [], summary: null,
  };
}

describe('DockDetails', () => {
  beforeEach(() => {
    appSettings.foldAbstract = false;
    identifyState.open = false;
    identifyState.paperId = null;
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        new Response(JSON.stringify(detail('info1')), {
          status: 200, headers: { 'content-type': 'application/json' },
        }),
      ),
    );
  });

  it('renders the title, identifier pills, and abstract', async () => {
    render(DockDetails, { props: { id: 'info1' } });
    expect(await screen.findByText('Attention')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /DOI/ })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /arXiv/ })).toBeInTheDocument();
    expect(screen.getByText(/dominant sequence transduction/)).toBeInTheDocument();
  });

  it('renders the editable Name row in the Record section', async () => {
    render(DockDetails, { props: { id: 'info1' } });
    await screen.findByText('Attention');
    expect(screen.getByText('Name')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Edit paper name' })).toBeInTheDocument();
  });

  it('collapses the abstract', async () => {
    render(DockDetails, { props: { id: 'info1' } });
    await screen.findByText('Attention');
    await userEvent.click(screen.getByRole('button', { name: /Abstract/ }));
    expect(screen.queryByText(/dominant sequence transduction/)).not.toBeInTheDocument();
  });

  it('renders the LLM summary when present', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        new Response(
          JSON.stringify({
            ...detail('info-summary'),
            summary: { tldr: 'Short.', problem: 'P', approach: 'A', results: 'R', limitations: 'L' },
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        ),
      ),
    );
    render(DockDetails, { props: { id: 'info-summary' } });
    expect(await screen.findByText('Short.')).toBeInTheDocument();
    expect(screen.getByText(/Results/i)).toBeInTheDocument();
  });

  it('starts the abstract folded when fold_abstract is true', async () => {
    appSettings.foldAbstract = true;
    render(DockDetails, { props: { id: 'info1' } });
    const toggle = await screen.findByRole('button', { name: /abstract/i });
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
  });

  it('does not render a Chat button even when chat is available', async () => {
    chat.available = true;
    try {
      render(DockDetails, { props: { id: 'info1' } });
      await screen.findByText('Attention');
      expect(screen.queryByRole('button', { name: /Chat/ })).not.toBeInTheDocument();
    } finally {
      chat.available = false;
    }
  });

  it('launches Identify from a direct button in the pane', async () => {
    render(DockDetails, { props: { id: 'info1' } });
    await screen.findByText('Attention');
    await userEvent.click(screen.getByRole('button', { name: /Identify/ }));
    expect(identifyState.open).toBe(true);
    expect(identifyState.paperId).toBe('info1');
  });

  it('shows a standalone Delete paper button and no overflow menu', async () => {
    render(DockDetails, { props: { id: 'info1' } });
    await screen.findByText('Attention');
    expect(screen.getByRole('button', { name: /Delete paper/ })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /More actions/ })).not.toBeInTheDocument();
  });
});
