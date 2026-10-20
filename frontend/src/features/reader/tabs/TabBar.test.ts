import { render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';
import TabBar from './TabBar.svelte';
import { closeTab, openTab, viewer } from './state.svelte';
import type { PaperSummary } from '$shared/api/generated/PaperSummary';

function paper(id: string, title: string, name: string | null = null): PaperSummary {
  return {
    id, title, authors: [], venue: null, year: null, doi: null, arxiv_id: null,
    dblp_key: null, cite_key: null, url: null, source: null, status: 'resolved',
    added_at: '', name, starred: false, tags: [], projects: [],
  };
}

describe('TabBar', () => {
  beforeEach(() => {
    viewer.tabs = [];
    viewer.activeId = null;
    viewer.zen = false;
  });

  it('renders one tab per open paper and closes them', async () => {
    openTab(paper('a', 'First Paper'));
    openTab(paper('b', 'Second Paper'));
    render(TabBar);
    expect(screen.getByText('First Paper')).toBeInTheDocument();
    expect(screen.getByText('Second Paper')).toBeInTheDocument();
    expect(viewer.tabs.length).toBe(2);
    expect(viewer.activeId).toBe('b'); // most-recently opened is active

    closeTab('b');
    expect(viewer.tabs.length).toBe(1);
    expect(viewer.activeId).toBe('a'); // falls back to a neighbor
  });

  it('always shows the Library home tab and returns home on click', async () => {
    openTab(paper('a', 'First Paper'));
    render(TabBar);
    const home = screen.getByRole('button', { name: 'Library' });
    expect(home).toBeInTheDocument();
    home.click();
    await Promise.resolve();
    expect(viewer.activeId).toBe(null);
    expect(viewer.tabs.length).toBe(1); // tabs survive going home
  });

  it('marks the home tab current when no PDF tab is active', () => {
    render(TabBar);
    expect(screen.getByRole('button', { name: 'Library' })).toHaveAttribute('aria-current', 'page');
  });

  it('keeps the close button visible on keyboard focus and titles truncated tabs', () => {
    openTab(paper('a', 'A very long paper title'));
    render(TabBar);
    expect(screen.getByTitle('A very long paper title')).toBeInTheDocument();
    const close = screen.getByRole('button', { name: 'Close tab' });
    expect(close.className).toContain('focus-visible:opacity-100');
  });

  it('balances the close button with a spacer so the label stays centered', () => {
    openTab(paper('a', 'Attention Is All You Need', 'Transformer'));
    render(TabBar);
    const close = screen.getByRole('button', { name: 'Close tab' });
    const spacer = close.parentElement!.firstElementChild!;
    expect(spacer.className).toContain('w-4');
    expect(close.className).toContain('w-4');
  });

  it('labels a named paper by its name, keeping the full title as the tooltip', () => {
    openTab(paper('a', 'Attention Is All You Need', 'Transformer'));
    render(TabBar);
    // The tooltip is the only place the full title can still be read, so it
    // must stay the title even though the label no longer is.
    const tab = screen.getByTitle('Attention Is All You Need');
    expect(tab.textContent?.trim()).toBe('Transformer');
    expect(tab.className).toContain('font-sans'); // same voice as the table's Name column
  });

  it('hosts no zen/info buttons (they live on the PDF toolbar)', async () => {
    openTab(paper('a', 'First Paper'));
    render(TabBar);
    expect(screen.queryByRole('button', { name: /zen/i })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Toggle info' })).not.toBeInTheDocument();
  });
});
