import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import ReaderDock from '$features/reader/dock/ReaderDock.svelte';
import { chat } from '$features/reader/ask/chat.svelte';
import { handleKeydown } from '$app/keys/shortcuts';
import { viewer } from '$features/reader/tabs/state.svelte';
import { appSettings } from '$shared/api/settings.svelte';
import { dock } from '$features/reader/dock/state.svelte';

const detail = {
  id: 'p1', title: 'Attention', authors: ['Vaswani'], venue: 'NeurIPS', year: 2017,
  doi: null, arxiv_id: null, dblp_key: null, cite_key: 'vaswani2017', url: null,
  source: null, status: 'resolved', added_at: '2026-07-08T00:00:00Z',
  abstract: 'Abs.', name: null, starred: false, tags: [], projects: [], summary: null,
};

beforeEach(() => {
  viewer.activeId = 'p1';
  dock.open = true;
  dock.entry = null;
  viewer.zen = false;
  appSettings.foldAbstract = false;
  chat.available = true;
  chat.models = [{ id: '0', label: 'Mock A' }];
  chat.modelId = '0';
  chat.paperId = 'p1';
  chat.messages = [];
  chat.pending = null;
  chat.streaming = null;
  chat.busy = false;
  chat.error = null;
  chat.draft = '';
  localStorage.clear();
  vi.stubGlobal(
    'fetch',
    vi.fn(async () =>
      new Response(JSON.stringify(detail), {
        status: 200, headers: { 'content-type': 'application/json' },
      }),
    ),
  );
});

describe('ReaderDock', () => {
  it('Escape inside the dock never reaches the global shortcut handler', async () => {
    viewer.zen = true;
    // Mount the real app-level keydown handler: without the dock's
    // stopPropagation it would see the dock already closed and exit zen.
    window.addEventListener('keydown', handleKeydown);
    try {
      render(ReaderDock, { props: { id: 'p1' } });
      await userEvent.click(screen.getByPlaceholderText('Ask about this paper…'));
      await userEvent.keyboard('{Escape}');
      expect(dock.open).toBe(false);
      expect(viewer.zen).toBe(true);
    } finally {
      window.removeEventListener('keydown', handleKeydown);
    }
  });
});
