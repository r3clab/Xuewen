import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import PaperNameEditor from './PaperNameEditor.svelte';
import type { PaperDetail } from '$shared/api/generated/PaperDetail';

function stubFetch(handler: (url: string, init?: RequestInit) => unknown) {
  const calls: Array<{ url: string; method?: string; body?: string }> = [];
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string | URL, init?: RequestInit) => {
      calls.push({ url: String(url), method: init?.method, body: init?.body as string | undefined });
      const body = handler(String(url), init);
      return new Response(JSON.stringify(body ?? {}), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      });
    }),
  );
  return calls;
}

function detail(overrides: Partial<PaperDetail> = {}): PaperDetail {
  return {
    id: 'p1', title: 't', authors: [], venue: null, year: null, doi: null, arxiv_id: null,
    dblp_key: null, cite_key: null, url: null, source: null, status: 'resolved', added_at: '',
    name: null, starred: false, tags: [], projects: [],
    abstract: null,
    ...overrides,
  };
}

describe('PaperNameEditor', () => {
  beforeEach(() => {
    vi.unstubAllGlobals();
  });

  it('shows the empty-state affordance and commits a typed name on Enter', async () => {
    const calls = stubFetch(() => ({ name: 'RVSpec' }));
    render(PaperNameEditor, { props: { d: detail() } });
    await userEvent.click(screen.getByRole('button', { name: 'Edit paper name' }));
    await userEvent.type(screen.getByLabelText('Paper name'), 'RVSpec{Enter}');
    const patch = calls.find((c) => c.method === 'PATCH');
    expect(patch?.url).toBe('/api/papers/p1/name');
    expect(JSON.parse(patch?.body ?? '{}')).toEqual({ name: 'RVSpec' });
    // Back to idle mode after the commit resolves.
    expect(await screen.findByRole('button', { name: 'Edit paper name' })).toBeInTheDocument();
  });

  it('shows the current name and clears it by committing an empty draft', async () => {
    const calls = stubFetch(() => ({ name: null }));
    render(PaperNameEditor, { props: { d: detail({ name: 'RVSpec' }) } });
    const btn = screen.getByRole('button', { name: 'Edit paper name' });
    expect(btn).toHaveTextContent('RVSpec');
    await userEvent.click(btn);
    await userEvent.clear(screen.getByLabelText('Paper name'));
    await userEvent.keyboard('{Enter}');
    const patch = calls.find((c) => c.method === 'PATCH');
    expect(JSON.parse(patch?.body ?? '{}')).toEqual({ name: null });
  });

  it('committing the unchanged value is a no-op without a network call', async () => {
    const calls = stubFetch(() => ({}));
    render(PaperNameEditor, { props: { d: detail({ name: 'RVSpec' }) } });
    await userEvent.click(screen.getByRole('button', { name: 'Edit paper name' }));
    await userEvent.keyboard('{Enter}'); // draft still "RVSpec"
    expect(calls).toHaveLength(0);
    expect(screen.getByRole('button', { name: 'Edit paper name' })).toBeInTheDocument();
  });

  it('Escape cancels the edit without a network call', async () => {
    const calls = stubFetch(() => ({}));
    render(PaperNameEditor, { props: { d: detail({ name: 'RVSpec' }) } });
    await userEvent.click(screen.getByRole('button', { name: 'Edit paper name' }));
    await userEvent.type(screen.getByLabelText('Paper name'), ' scrapped');
    await userEvent.keyboard('{Escape}');
    expect(calls).toHaveLength(0);
    expect(screen.getByRole('button', { name: 'Edit paper name' })).toHaveTextContent('RVSpec');
  });

  it('a rejected save shows an inline error and keeps the draft editable', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify({ error: 'name is too long' }), { status: 400 })),
    );
    render(PaperNameEditor, { props: { d: detail() } });
    await userEvent.click(screen.getByRole('button', { name: 'Edit paper name' }));
    await userEvent.type(screen.getByLabelText('Paper name'), 'Whoops{Enter}');
    expect(await screen.findByText(/name is too long|update name failed/)).toBeInTheDocument();
    // Still in edit mode, draft intact for a retry.
    expect(screen.getByLabelText('Paper name')).toHaveValue('Whoops');
  });
});
