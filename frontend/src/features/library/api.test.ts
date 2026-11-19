import { afterEach, describe, expect, it, vi } from 'vitest';
import { exportPapers, listPapers } from './api';

function stubFetch(body: BodyInit = '[]') {
  const fetch = vi.fn(async () => new Response(body));
  vi.stubGlobal('fetch', fetch);
  return fetch;
}

afterEach(() => vi.unstubAllGlobals());

describe('listPapers', () => {
  it('sends the raw query string and the sort, and nothing else', async () => {
    const fetch = stubFetch();
    await listPapers({ q: '  tag:nlp project:Thesis  ', sort: 'title' });
    expect(fetch).toHaveBeenCalledWith('/api/papers?q=tag%3Anlp+project%3AThesis&sort=title', expect.anything());
  });

  it('omits an empty query and the default order', async () => {
    const fetch = stubFetch();
    await listPapers({ q: '   ' });
    expect(fetch).toHaveBeenCalledWith('/api/papers?', expect.anything());
  });
});

describe('exportPapers', () => {
  it('posts exactly the given ids in order, with the format', async () => {
    const fetch = stubFetch('@article{a,}');
    const blob = await exportPapers({ ids: ['b', 'a'], format: 'biblatex' });
    expect(await blob.text()).toBe('@article{a,}');
    expect(fetch).toHaveBeenCalledWith('/api/papers/export', {
      method: 'POST',
      signal: undefined,
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ ids: ['b', 'a'], format: 'biblatex' }),
    });
  });
});
