import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock(import('$features/library/api'), async (importOriginal) => {
  const mod = await importOriginal<typeof import('$features/library/api')>();
  const { ApiError } = await import('$shared/api/http');
  return {
    ...mod,
    getPaper: vi.fn(async (id: string) => {
      // The server positively saying "gone" — the only rejection that prunes.
      if (id === 'dead') throw new ApiError('paper not found', 404);
      // Transient failures: the request never got an answer / a 500.
      if (id === 'offline') throw new Error('fetch failed');
      if (id === 'flaky') throw new ApiError('detail failed: 500', 500);
      return { id } as never;
    }),
  };
});

import { activateTab, closeTab, goHome, initTabs, openTab, viewer } from './state.svelte';
import type { PaperSummary } from '$shared/api/generated/PaperSummary';

const TABS_KEY = 'xuewen-tabs';

function paper(id: string, title: string, name: string | null = null): PaperSummary {
  return {
    id, title, authors: [], venue: null, year: null, doi: null, arxiv_id: null,
    dblp_key: null, cite_key: null, url: null, source: null, status: 'resolved',
    added_at: '', name, starred: false, tags: [], projects: [],
  };
}

function saved(): {
  tabs: { id: string; title: string; name: string | null }[];
  activeId: string | null;
} {
  return JSON.parse(localStorage.getItem(TABS_KEY)!);
}

beforeEach(() => {
  localStorage.clear();
  viewer.tabs = [];
  viewer.activeId = null;
});

describe('tab persistence', () => {
  it('open/activate/close/goHome all write the tab set to storage', () => {
    openTab(paper('a', 'A'));
    openTab(paper('b', 'B'));
    expect(saved()).toEqual({
      tabs: [
        { id: 'a', title: 'A', name: null },
        { id: 'b', title: 'B', name: null },
      ],
      activeId: 'b',
    });
    activateTab('a');
    expect(saved().activeId).toBe('a');
    expect(viewer.activeId).toBe('a');
    goHome();
    expect(saved().activeId).toBe(null);
    closeTab('a');
    expect(saved().tabs).toEqual([{ id: 'b', title: 'B', name: null }]);
  });

  it("carries the paper's name onto the tab, and through storage", async () => {
    openTab(paper('a', 'Attention Is All You Need', 'Transformer'));
    expect(viewer.tabs[0].name).toBe('Transformer');
    expect(saved().tabs).toEqual([
      { id: 'a', title: 'Attention Is All You Need', name: 'Transformer' },
    ]);
    viewer.tabs = [];
    await initTabs();
    expect(viewer.tabs[0].name).toBe('Transformer');
  });

  it('initTabs restores tabs and the active tab, defaulting a missing name', async () => {
    // No `name` key: exactly what a build from before the field wrote. Those
    // tabs must restore (with name null), not be rejected as malformed.
    localStorage.setItem(
      TABS_KEY,
      JSON.stringify({ tabs: [{ id: 'a', title: 'A' }], activeId: 'a' }),
    );
    await initTabs();
    expect(viewer.tabs).toEqual([{ id: 'a', title: 'A', name: null }]);
    expect(viewer.activeId).toBe('a');
  });

  it('initTabs drops tabs whose papers no longer exist', async () => {
    localStorage.setItem(
      TABS_KEY,
      JSON.stringify({
        tabs: [
          { id: 'a', title: 'A' },
          { id: 'dead', title: 'Gone' },
        ],
        activeId: 'dead',
      }),
    );
    await initTabs();
    expect(viewer.tabs.map((t) => t.id)).toEqual(['a']);
    expect(viewer.activeId).toBe(null); // the active tab died → land on home
    expect(saved().tabs.map((t) => t.id)).toEqual(['a']); // pruned set re-saved
  });

  it('initTabs keeps tabs whose validation failed transiently (network, 5xx)', async () => {
    // Loading the UI while the backend restarts must not wipe the remembered
    // workspace: only a definite 404/410 may prune, never a failed request.
    localStorage.setItem(
      TABS_KEY,
      JSON.stringify({
        tabs: [
          { id: 'a', title: 'A' },
          { id: 'offline', title: 'B' },
          { id: 'flaky', title: 'C' },
        ],
        activeId: 'flaky',
      }),
    );
    await initTabs();
    expect(viewer.tabs.map((t) => t.id)).toEqual(['a', 'offline', 'flaky']);
    expect(viewer.activeId).toBe('flaky');
    expect(saved().tabs.map((t) => t.id)).toEqual(['a', 'offline', 'flaky']); // not re-saved pruned
  });

  it('initTabs tolerates corrupted or missing storage', async () => {
    localStorage.setItem(TABS_KEY, '{nope');
    await initTabs();
    expect(viewer.tabs).toEqual([]);
    localStorage.removeItem(TABS_KEY);
    await initTabs();
    expect(viewer.tabs).toEqual([]);
  });
});
