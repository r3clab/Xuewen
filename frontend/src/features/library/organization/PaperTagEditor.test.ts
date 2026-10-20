import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import PaperTagEditor from './PaperTagEditor.svelte';
import { tags } from './state.svelte';
import type { PaperDetail } from '$shared/api/generated/PaperDetail';

function stubFetch(handler: (url: string, init?: RequestInit) => unknown) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string | URL, init?: RequestInit) => {
      const body = handler(String(url), init);
      return new Response(JSON.stringify(body ?? {}), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      });
    }),
  );
}

function detail(overrides: Partial<PaperDetail> = {}): PaperDetail {
  return {
    id: 'p1', title: 't', authors: [], venue: null, year: null, doi: null, arxiv_id: null,
    dblp_key: null, cite_key: null, url: null, source: null, status: 'resolved', added_at: '',
    name: null, starred: false, tags: [{ id: 't1', name: 'security/fuzzing' }], projects: [],
    abstract: null,
    ...overrides,
  };
}

describe('PaperTagEditor', () => {
  beforeEach(() => {
    tags.items = [];
    vi.unstubAllGlobals();
  });

  it('adds a new tag via Enter on free text', async () => {
    const calls: Array<{ url: string; method?: string; body?: string }> = [];
    stubFetch((url, init) => {
      calls.push({ url, method: init?.method, body: init?.body as string | undefined });
      if (url.endsWith('/tags') && init?.method === 'PUT') return { id: 'new1', name: 'ml/rl' };
      return [];
    });
    render(PaperTagEditor, { props: { d: detail() } });
    await userEvent.type(screen.getByLabelText('Add a tag'), 'ml/rl{Enter}');
    const put = calls.find((c) => c.method === 'PUT');
    expect(put).toBeTruthy();
    expect(put?.url).toBe('/api/papers/p1/tags');
    expect(put?.body).toContain('ml/rl');
  });

  it('suggests existing tags by substring and adds the highlighted one on Enter', async () => {
    tags.items = [
      { id: 'tA', name: 'ml/llm', paper_count: 2, created_at: '' },
      { id: 'tB', name: 'os/rtos', paper_count: 1, created_at: '' },
    ];
    const calls: Array<{ url: string; method?: string; body?: string }> = [];
    stubFetch((url, init) => {
      calls.push({ url, method: init?.method, body: init?.body as string | undefined });
      if (url.endsWith('/tags') && init?.method === 'PUT') return { id: 'tA', name: 'ml/llm' };
      return [];
    });
    render(PaperTagEditor, { props: { d: detail() } });
    const input = screen.getByLabelText('Add a tag');
    await userEvent.type(input, 'ml');
    expect(screen.getByRole('button', { name: 'ml/llm' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'os/rtos' })).not.toBeInTheDocument();
    await userEvent.keyboard('{ArrowDown}{Enter}');
    const put = calls.find((c) => c.method === 'PUT');
    expect(put?.body).toContain('ml/llm');
  });

  it('removes a tag via the ✕ button', async () => {
    const calls: Array<{ url: string; method?: string }> = [];
    stubFetch((url, init) => {
      calls.push({ url, method: init?.method });
      return [];
    });
    render(PaperTagEditor, { props: { d: detail() } });
    await userEvent.click(screen.getByRole('button', { name: 'Remove tag security/fuzzing' }));
    expect(calls.some((c) => c.url === '/api/papers/p1/tags/t1' && c.method === 'DELETE')).toBe(
      true,
    );
  });
});
