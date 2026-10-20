import { beforeEach, describe, expect, it, vi } from 'vitest';
import { annotationTools, setActiveTool } from '$features/reader/annotations/state.svelte';
import {
  closeTab,
  goHome,
  openTab,
  revealLibrary,
  selection,
  selectPaper,
  toggleZen,
  viewer,
} from './state.svelte';
import type { PaperSummary } from '$shared/api/generated/PaperSummary';

function paper(id: string): PaperSummary {
  return {
    id, title: id, authors: [], venue: null, year: null, doi: null, arxiv_id: null,
    dblp_key: null, cite_key: null, url: null, source: null, status: 'resolved',
    added_at: '', name: null, starred: false, tags: [], projects: [],
  };
}

beforeEach(() => {
  viewer.tabs = [];
  viewer.activeId = null;
  selection.id = null;
  viewer.zen = false;
  annotationTools.active = null;
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

describe('selection and home tab', () => {
  it('selectPaper sets and clears the browsing selection', () => {
    selectPaper('a');
    expect(selection.id).toBe('a');
    selectPaper(null);
    expect(selection.id).toBe(null);
  });

  it('openTab activates the tab and selects the paper', () => {
    openTab(paper('a'));
    expect(viewer.activeId).toBe('a');
    expect(selection.id).toBe('a');
  });

  it('goHome keeps tabs open but activates the Library home', () => {
    openTab(paper('a'));
    goHome();
    expect(viewer.activeId).toBe(null);
    expect(viewer.tabs.length).toBe(1);
  });

  it('revealLibrary leaves the reader for the Library, and is a no-op at home', () => {
    openTab(paper('a'));
    viewer.zen = true;
    revealLibrary();
    expect(viewer.activeId).toBe(null);
    expect(viewer.tabs).toHaveLength(1);
    expect(viewer.zen).toBe(false);
    revealLibrary();
    expect(viewer.activeId).toBe(null);
  });

  it('closing the last tab lands on the Library home', () => {
    openTab(paper('a'));
    closeTab('a');
    expect(viewer.tabs.length).toBe(0);
    expect(viewer.activeId).toBe(null);
  });
});

describe('zen mode', () => {
  it('toggleZen only engages while a PDF tab is active', () => {
    toggleZen();
    expect(viewer.zen).toBe(false); // home active — nothing to zen into
    openTab(paper('a'));
    toggleZen();
    expect(viewer.zen).toBe(true);
    toggleZen();
    expect(viewer.zen).toBe(false);
  });

  it('closing the last tab exits zen', () => {
    openTab(paper('a'));
    toggleZen();
    closeTab('a');
    expect(viewer.zen).toBe(false);
  });

  it('closing the last tab disarms the annotation tool', () => {
    // A tool armed with no reader on screen would silently re-arm on the next
    // opened tab (AnnotationTools re-applies the persisted tool per document).
    openTab(paper('a'));
    openTab(paper('b'));
    setActiveTool('highlight');
    closeTab('a');
    expect(annotationTools.active).toBe('highlight'); // a reader is still up
    closeTab('b');
    expect(annotationTools.active).toBeNull();
  });

  it('goHome exits zen', () => {
    openTab(paper('a'));
    toggleZen();
    goHome();
    expect(viewer.zen).toBe(false);
  });
});
