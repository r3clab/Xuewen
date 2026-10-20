import { beforeEach, describe, expect, it, vi, type Mock } from 'vitest';

vi.mock(import('$features/library/api'), async (importOriginal) => {
  const mod = await importOriginal<typeof import('$features/library/api')>();
  return {
    ...mod,
    deletePaper: vi.fn(async () => {}),
    restorePaper: vi.fn(async () => {}),
    listPapers: vi.fn(async () => []),
    getStats: vi.fn(async () => ({ total: 0, resolved: 0, needs_review: 0 })),
    // Echoes the name back the way the server does (it is authoritative about
    // trimming and empty -> null; nothing here exercises that).
    setPaperName: vi.fn(async (_id: string, name: string | null) => ({ name }) as never),
  };
});

import * as api from '$features/library/api';
import { library } from '$features/library/state.svelte';
import { removePaper, removePapers, setPaperName } from './actions';
import { toasts } from '$shared/ui/toasts.svelte';
import { openTab, selection, selectPaper, viewer } from '$features/reader/tabs/state.svelte';
import type { PaperSummary } from '$shared/api/generated/PaperSummary';

function paper(id: string, title = 'T'): PaperSummary {
  return {
    id, title, authors: [], venue: null, year: null, doi: null, arxiv_id: null,
    dblp_key: null, cite_key: null, url: null, source: null, status: 'resolved',
    added_at: '', name: null, starred: false, tags: [], projects: [],
  };
}

const TABS_KEY = 'xuewen-tabs';

function saved(): {
  tabs: { id: string; title: string; name: string | null }[];
  activeId: string | null;
} {
  return JSON.parse(localStorage.getItem(TABS_KEY)!);
}

beforeEach(() => {
  vi.clearAllMocks();
  library.papers = [paper('p1')];
  toasts.items.length = 0;
});

describe('paper delete undo', () => {
  it('removePaper shows a Deleted toast with an Undo action', async () => {
    await removePaper('p1');
    const t = toasts.items.find((x) => x.action);
    expect(t?.message).toMatch(/deleted/i);
    expect(t?.action?.label).toBe('Undo');
  });

  it('removePapers deletes each id and shows ONE combined Undo toast', async () => {
    library.papers = [paper('p1'), paper('p2')];
    await removePapers(['p1', 'p2']);
    expect((api.deletePaper as Mock).mock.calls.map(([id]) => id)).toEqual(['p1', 'p2']);
    const undoToasts = toasts.items.filter((x) => x.action);
    expect(undoToasts).toHaveLength(1);
    expect(undoToasts[0].message).toMatch(/2 papers deleted/i);
    undoToasts[0].action!.run();
    await vi.waitFor(() => {
      expect((api.restorePaper as Mock).mock.calls.map(([id]) => id)).toEqual(['p1', 'p2']);
    });
  });

  it('a partial failure gets an error toast and an Undo covering only the deleted ids', async () => {
    library.papers = [paper('p1'), paper('p2')];
    (api.deletePaper as Mock)
      .mockImplementationOnce(async () => {}) // p1 goes through
      .mockRejectedValueOnce(new Error('boom')); // p2 fails
    await removePapers(['p1', 'p2']);

    expect(library.papers.map((p) => p.id)).toEqual(['p2']); // the failure stays put
    expect(toasts.items.some((x) => /couldn't delete/i.test(x.message))).toBe(true);
    const undoToast = toasts.items.find((x) => x.action);
    expect(undoToast?.message).toBe('Paper deleted'); // counts only what succeeded
    undoToast!.action!.run();
    await vi.waitFor(() => {
      // Undo must not "restore" the never-deleted p2.
      expect((api.restorePaper as Mock).mock.calls.map(([id]) => id)).toEqual(['p1']);
    });
  });

  it('an all-failed delete rejects nothing and shows no Undo', async () => {
    (api.deletePaper as Mock).mockRejectedValueOnce(new Error('boom'));
    await removePapers(['p1']); // must not throw (LibraryTable's run has no catch)
    expect(library.papers.map((p) => p.id)).toEqual(['p1']);
    expect(toasts.items.filter((x) => x.action)).toHaveLength(0);
    expect(toasts.items.some((x) => /couldn't delete/i.test(x.message))).toBe(true);
  });

  it('running Undo restores the paper and reloads the list', async () => {
    await removePaper('p1');
    toasts.items.find((x) => x.action)!.action!.run();
    await vi.waitFor(() => {
      expect(api.restorePaper as Mock).toHaveBeenCalledWith('p1');
      expect(api.listPapers as Mock).toHaveBeenCalled();
    });
    expect(toasts.items.some((x) => /restored/i.test(x.message))).toBe(true);
  });
});

describe('removePaper selection', () => {
  it('clears the selection when the selected paper is deleted', async () => {
    library.papers = [paper('x')];
    selectPaper('x');
    await removePaper('x');
    expect(selection.id).toBe(null);
  });
});

describe('setPaperName', () => {
  beforeEach(() => {
    localStorage.clear();
    viewer.tabs = [];
    viewer.activeId = null;
  });

  it('relabels an already-open tab when the paper is renamed', async () => {
    openTab(paper('a', 'Attention Is All You Need'));
    await setPaperName('a', 'Transformer');
    expect(viewer.tabs[0].name).toBe('Transformer');
    expect(saved().tabs[0].name).toBe('Transformer'); // and the new label survives a reload
  });
});
