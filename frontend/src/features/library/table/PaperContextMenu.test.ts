import { render, screen, waitFor } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import PaperContextMenu from './PaperContextMenu.svelte';
import { closeContextMenu, contextMenu, openContextMenu } from './contextMenu.svelte';
import { dismissTopLayer } from '$shared/ui/escape';
import { identifyState } from '$features/identify/state.svelte';
import { library } from '$features/library/state.svelte';
import { selection, viewer } from '$features/reader/tabs/state.svelte';
import { toasts } from '$shared/ui/toasts.svelte';
import type { PaperSummary } from '$shared/api/generated/PaperSummary';

const paper: PaperSummary = {
  id: 'p1', title: 'Attention Is All You Need', authors: ['Vaswani'], venue: 'NeurIPS',
  year: 2017, doi: '10.1/x', arxiv_id: '1706.03762', dblp_key: null, cite_key: 'v2017', url: null,
  source: null, status: 'resolved', added_at: '', name: null, starred: false, tags: [], projects: [],
};

// Branch the fetch mock by URL/method so copyCitation (export text), delete
// (DELETE ok), and the loadStats() that removePaper triggers each get a sane
// response.
function stubFetch() {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, opts?: { method?: string }) => {
      const u = String(url);
      if (u.includes('/export')) return new Response('@article{v2017}', { status: 200 });
      void opts;
      return new Response('{}', { status: 200, headers: { 'content-type': 'application/json' } });
    }),
  );
}

function openAt(target: PaperSummary, x = 10, y = 10): void {
  openContextMenu(new MouseEvent('contextmenu', { clientX: x, clientY: y, cancelable: true }), target);
}

beforeEach(() => {
  window.addEventListener('keydown', dismissTopLayer);
  return () => window.removeEventListener('keydown', dismissTopLayer);
});

beforeEach(() => {
  toasts.items.length = 0;
  selection.id = null;
  viewer.tabs = [];
  identifyState.open = false;
  identifyState.paperId = null;
  library.papers = [paper];
  openAt(paper);
  vi.unstubAllGlobals();
  stubFetch();
  vi.stubGlobal('navigator', { clipboard: { writeText: vi.fn(async () => {}) } });
});

describe('PaperContextMenu', () => {
  it('renders the three actions for the target paper', () => {
    render(PaperContextMenu);
    expect(screen.getByRole('menuitem', { name: /copy bibtex/i })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: /identify/i })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: /delete/i })).toBeInTheDocument();
  });

  it('Copy BibTeX copies to the clipboard, toasts, and closes the menu', async () => {
    render(PaperContextMenu);
    await userEvent.click(screen.getByRole('menuitem', { name: /copy bibtex/i }));
    await waitFor(() => {
      expect((navigator.clipboard.writeText as ReturnType<typeof vi.fn>)).toHaveBeenCalledWith(
        '@article{v2017}',
      );
    });
    expect(toasts.items.some((t) => t.kind === 'success')).toBe(true);
    expect(contextMenu.paper).toBeNull();
  });

  it('Identify opens the identify modal for the paper and closes the menu', async () => {
    render(PaperContextMenu);
    await userEvent.click(screen.getByRole('menuitem', { name: /identify/i }));
    expect(identifyState.open).toBe(true);
    expect(identifyState.paperId).toBe('p1');
    expect(contextMenu.paper).toBeNull();
  });

  it('Delete requires a confirm before hitting the API, then closes', async () => {
    render(PaperContextMenu);
    await userEvent.click(screen.getByRole('menuitem', { name: /delete/i }));
    // First click only reveals the confirm — no DELETE yet.
    expect(globalThis.fetch as ReturnType<typeof vi.fn>).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => {
      const calls = (globalThis.fetch as ReturnType<typeof vi.fn>).mock.calls;
      expect(calls.some(([u, o]) => String(u).includes('/api/papers/p1') && o?.method === 'DELETE')).toBe(
        true,
      );
    });
    expect(contextMenu.paper).toBeNull();
  });

  it('the delete confirm takes focus, so Enter confirms', async () => {
    render(PaperContextMenu);
    await userEvent.click(screen.getByRole('menuitem', { name: /delete/i }));
    expect(screen.getByRole('button', { name: 'Delete' })).toHaveFocus();
  });

  it('a retarget mid-confirm lands on the action list, like a fresh open', async () => {
    render(PaperContextMenu);
    await userEvent.click(screen.getByRole('menuitem', { name: /delete/i }));
    openAt({ ...paper, id: 'p2' }, 40, 40);
    await waitFor(() => expect(screen.getByRole('menuitem', { name: /copy bibtex/i })).toHaveFocus());
    expect(screen.queryByRole('button', { name: 'Delete' })).not.toBeInTheDocument();
  });

  it('Escape closes the menu', async () => {
    render(PaperContextMenu);
    await userEvent.keyboard('{Escape}');
    expect(contextMenu.paper).toBeNull();
  });

  it('dismisses when a pane scrolls under it', async () => {
    render(PaperContextMenu);
    await waitFor(() =>
      expect(screen.getByRole('menuitem', { name: /copy bibtex/i })).toHaveFocus(),
    );
    // The list panes' scroll events don't bubble to window — the shell's
    // capture-phase listener is what keeps the menu from floating detached
    // from its row when the pane scrolls.
    const pane = document.createElement('div');
    document.body.appendChild(pane);
    pane.dispatchEvent(new Event('scroll'));
    expect(contextMenu.paper).toBeNull();
    pane.remove();
  });

  it('renders nothing when closed', () => {
    closeContextMenu();
    render(PaperContextMenu);
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });
});

describe('PaperContextMenu keyboard', () => {
  it('focuses the first menuitem on open', async () => {
    render(PaperContextMenu);
    await waitFor(() =>
      expect(screen.getByRole('menuitem', { name: /copy bibtex/i })).toHaveFocus(),
    );
  });

  it('ArrowDown/ArrowUp rove through items with wrap-around, Home/End jump', async () => {
    render(PaperContextMenu);
    await waitFor(() =>
      expect(screen.getByRole('menuitem', { name: /copy bibtex/i })).toHaveFocus(),
    );
    await userEvent.keyboard('{ArrowDown}');
    expect(screen.getByRole('menuitem', { name: /identify/i })).toHaveFocus();
    await userEvent.keyboard('{ArrowDown}');
    expect(screen.getByRole('menuitem', { name: /delete/i })).toHaveFocus();
    await userEvent.keyboard('{ArrowDown}'); // wraps to the top
    expect(screen.getByRole('menuitem', { name: /copy bibtex/i })).toHaveFocus();
    await userEvent.keyboard('{ArrowUp}'); // wraps to the bottom
    expect(screen.getByRole('menuitem', { name: /delete/i })).toHaveFocus();
    await userEvent.keyboard('{Home}');
    expect(screen.getByRole('menuitem', { name: /copy bibtex/i })).toHaveFocus();
    await userEvent.keyboard('{End}');
    expect(screen.getByRole('menuitem', { name: /delete/i })).toHaveFocus();
  });

  it('Enter activates the focused item', async () => {
    render(PaperContextMenu);
    await waitFor(() =>
      expect(screen.getByRole('menuitem', { name: /copy bibtex/i })).toHaveFocus(),
    );
    await userEvent.keyboard('{ArrowDown}{Enter}'); // Identify…
    expect(identifyState.open).toBe(true);
    expect(contextMenu.paper).toBeNull();
  });

  it('restores focus to the previously focused element on close', async () => {
    closeContextMenu();
    const outside = document.createElement('button');
    document.body.appendChild(outside);
    outside.focus();
    render(PaperContextMenu);
    openAt(paper);
    await waitFor(() =>
      expect(screen.getByRole('menuitem', { name: /copy bibtex/i })).toHaveFocus(),
    );
    await userEvent.keyboard('{Escape}');
    await waitFor(() => expect(outside).toHaveFocus());
    outside.remove();
  });
});
