import { beforeEach, describe, expect, it, vi } from 'vitest';
import { deleteTag, loadProjects, removeProject, renameProject, renameTag } from './actions';
import { library } from '$features/library/state.svelte';
import { loadDetail } from '$features/library/actions';
import { projects, tags } from './state.svelte';
import { filters } from '$features/library/search/state.svelte';
import { setProjectFilter, setStarFilter, setTagFilter } from '$features/library/search/actions';

function stubFetch(handler: (url: string, init?: RequestInit) => unknown) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string | URL, init?: RequestInit) => {
      const body = handler(String(url), init);
      return new Response(JSON.stringify(body), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      });
    }),
  );
}

describe('projects state', () => {
  beforeEach(() => {
    projects.items = [];
    filters.q = '';
    filters.project = 'all';
    filters.tag = undefined;
    filters.starred = undefined;
    library.papers = [];
    vi.unstubAllGlobals();
  });

  it('loads projects', async () => {
    stubFetch((url) => {
      if (url === '/api/projects') return [{ id: 'p1', name: 'Survey', paper_count: 2 }];
      return [];
    });
    await loadProjects();
    expect(projects.items).toHaveLength(1);
    expect(projects.items[0].name).toBe('Survey');
  });

  it('setProjectFilter sends the project query param', async () => {
    let lastUrl = '';
    stubFetch((url) => {
      lastUrl = url;
      return [];
    });
    await setProjectFilter('p1');
    expect(filters.project).toBe('p1');
    expect(lastUrl).toContain('project=p1');
  });

  it('the project/tag/starred filters combine via query qualifiers', async () => {
    stubFetch(() => []);

    await setProjectFilter('p1');
    expect(filters.project).toBe('p1');
    expect(filters.q).toContain('project:p1');

    await setTagFilter('security');
    expect(filters.tag).toBe('security');
    expect(filters.project).toBe('p1'); // filters AND together now

    await setStarFilter(true);
    expect(filters.starred).toBe(true);
    expect(filters.tag).toBe('security');

    // toggling each qualifier off removes only that filter
    await setProjectFilter('all');
    expect(filters.project).toBe('all');
    expect(filters.tag).toBe('security');
    await setTagFilter(undefined);
    expect(filters.tag).toBeUndefined();
    expect(filters.starred).toBe(true);
    await setStarFilter(false);
    expect(filters.q).toBe('');
  });

  it('setTagFilter and setStarFilter send the matching query params', async () => {
    let lastUrl = '';
    stubFetch((url) => {
      lastUrl = url;
      return [];
    });
    await setTagFilter('ml');
    expect(lastUrl).toContain('tag=ml');
    await setStarFilter(true);
    expect(lastUrl).toContain('starred=true');
  });
});

describe('global rename/delete clears the per-paper detail cache', () => {
  beforeEach(() => {
    projects.items = [];
    tags.items = [];
    filters.q = '';
    filters.project = 'all';
    filters.tag = undefined;
    filters.starred = undefined;
    library.papers = [];
    vi.unstubAllGlobals();
  });

  function stubDetailAnd(handler: (url: string, init?: RequestInit) => unknown) {
    stubFetch((url, init) => {
      if (url.startsWith('/api/papers/') && !url.includes('/projects/') && !url.includes('/tags')) {
        return { id: 'x', title: 'X', authors: [], venue: null, year: null, doi: null,
          arxiv_id: null, dblp_key: null, cite_key: null, url: null, source: null,
          status: 'resolved', added_at: '', name: null, starred: false, tags: [], projects: [], summary: null };
      }
      return handler(url, init);
    });
  }

  it('renameProject and removeProject evict the cached detail', async () => {
    stubDetailAnd(() => ({ id: 'p1', name: 'Survey', paper_count: 1 }));
    await loadDetail('x');
    const fetchMock = globalThis.fetch as ReturnType<typeof vi.fn>;
    const callsBefore = fetchMock.mock.calls.length;

    await renameProject('p1', { name: 'Renamed' });
    await loadDetail('x'); // must hit the network again: cache was cleared
    expect(fetchMock.mock.calls.length).toBeGreaterThan(callsBefore);

    await removeProject('p1');
    const callsBefore2 = fetchMock.mock.calls.length;
    await loadDetail('x');
    expect(fetchMock.mock.calls.length).toBeGreaterThan(callsBefore2);
  });

  it('renameTag and deleteTag evict the cached detail', async () => {
    stubDetailAnd(() => []);
    await loadDetail('x');
    const fetchMock = globalThis.fetch as ReturnType<typeof vi.fn>;
    const callsBefore = fetchMock.mock.calls.length;

    await renameTag('t1', 'renamed');
    await loadDetail('x'); // must hit the network again: cache was cleared
    expect(fetchMock.mock.calls.length).toBeGreaterThan(callsBefore);

    await deleteTag('t1');
    const callsBefore2 = fetchMock.mock.calls.length;
    await loadDetail('x');
    expect(fetchMock.mock.calls.length).toBeGreaterThan(callsBefore2);
  });
});

/// The search-box string is the single source of truth for the list filters;
/// a rename/delete that only patched the cached filters would leave a dead
/// qualifier in the box for the next sync to resurrect.
describe('tag/project rename+delete keep the search string in sync', () => {
  beforeEach(() => {
    projects.items = [];
    tags.items = [];
    filters.q = '';
    filters.project = 'all';
    filters.tag = undefined;
    filters.starred = undefined;
    library.papers = [];
    vi.unstubAllGlobals();
  });

  it('deleteTag drops the dead tag: qualifier from the query string', async () => {
    stubFetch(() => []);
    tags.items = [{ id: 't1', name: 'nlp', paper_count: 1, created_at: '' }];
    await setTagFilter('nlp');
    expect(filters.q).toContain('tag:nlp');

    await deleteTag('t1');
    expect(filters.q).not.toContain('tag:');
    expect(filters.tag).toBeUndefined();
  });

  it('renameTag rewrites an active tag: qualifier to the new name', async () => {
    stubFetch((url, init) => {
      if (init?.method === 'PATCH') return { id: 't1', name: 'ml' };
      return url === '/api/tags' ? [{ id: 't1', name: 'ml', paper_count: 1, created_at: '' }] : [];
    });
    tags.items = [{ id: 't1', name: 'nlp', paper_count: 1, created_at: '' }];
    await setTagFilter('nlp');

    await renameTag('t1', 'ml');
    expect(filters.q).toContain('tag:ml');
    expect(filters.tag).toBe('ml'); // the filter follows the rename
  });

  it('renameTag rewrites the qualifier with the server-normalized name, not the typed one', async () => {
    // The server normalizes tag names on rename ('nlp / eval' is stored as
    // 'nlp/eval') and the tag filter is an exact name match — writing the raw
    // typed name into the query would leave a qualifier matching nothing.
    stubFetch((url, init) => {
      if (init?.method === 'PATCH') return { id: 't1', name: 'nlp/eval' };
      return url === '/api/tags'
        ? [{ id: 't1', name: 'nlp/eval', paper_count: 1, created_at: '' }]
        : [];
    });
    tags.items = [{ id: 't1', name: 'nlp', paper_count: 1, created_at: '' }];
    await setTagFilter('nlp');

    await renameTag('t1', 'nlp / eval');
    expect(filters.q).toContain('tag:nlp/eval');
    expect(filters.tag).toBe('nlp/eval');
  });

  it('an inactive tag filter is left alone by deleteTag', async () => {
    stubFetch(() => []);
    tags.items = [
      { id: 't1', name: 'nlp', paper_count: 1, created_at: '' },
      { id: 't2', name: 'security', paper_count: 1, created_at: '' },
    ];
    await setTagFilter('security');

    await deleteTag('t1');
    expect(filters.q).toContain('tag:security');
    expect(filters.tag).toBe('security');
  });

  it('removeProject drops the dead project: qualifier from the query string', async () => {
    stubFetch(() => []);
    projects.items = [{ id: 'p1', name: 'Survey', paper_count: 1, created_at: '' }];
    await setProjectFilter('p1');
    expect(filters.q).toContain('project:Survey');

    await removeProject('p1');
    expect(filters.q).not.toContain('project:');
    expect(filters.project).toBe('all');
  });

  it('renameProject rewrites an active project: qualifier to the new name', async () => {
    stubFetch((url) =>
      url === '/api/projects' ? [{ id: 'p1', name: 'Renamed', paper_count: 1 }] : [],
    );
    projects.items = [{ id: 'p1', name: 'Survey', paper_count: 1, created_at: '' }];
    await setProjectFilter('p1');

    await renameProject('p1', { name: 'Renamed' });
    expect(filters.q).toContain('project:Renamed');
    expect(filters.project).toBe('p1'); // the new name resolves back to the id
  });
});
