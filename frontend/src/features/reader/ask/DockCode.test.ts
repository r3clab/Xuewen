import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import DockCode from './DockCode.svelte';

function codeResponse(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });
}

describe('DockCode', () => {
  it('shows the paste form when nothing is attached, then attaches', async () => {
    const fetchSpy = vi
      .fn()
      .mockResolvedValueOnce(codeResponse({ attached: false, code: null }))
      .mockResolvedValueOnce(codeResponse({ attached: true, code: { paper_id: 'p1', repo_url: 'https://github.com/x/y', commit_sha: null, status: 'cloning', error: null, cloned_at: null, size_bytes: null } }, 202))
      .mockResolvedValue(codeResponse({ attached: true, code: { paper_id: 'p1', repo_url: 'https://github.com/x/y', commit_sha: 'abc1234', status: 'ready', error: null, cloned_at: 'now', size_bytes: 1 } }));
    vi.stubGlobal('fetch', fetchSpy);
    render(DockCode, { props: { id: 'p1' } });
    const input = await screen.findByPlaceholderText('https://github.com/…');
    await userEvent.type(input, 'https://github.com/x/y');
    await userEvent.click(screen.getByRole('button', { name: 'Attach' }));
    expect(await screen.findByText(/cloning|abc1234/)).toBeInTheDocument();
  });

  it('asks for confirmation before detaching the repo', async () => {
    const ready = { paper_id: 'p1', repo_url: 'https://github.com/x/y', commit_sha: 'abc1234', status: 'ready', error: null, cloned_at: 'now', size_bytes: 1 };
    const fetchSpy = vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
      if (init?.method === 'DELETE') return new Response(null, { status: 204 });
      return codeResponse({ attached: true, code: ready });
    });
    vi.stubGlobal('fetch', fetchSpy);
    render(DockCode, { props: { id: 'p1' } });
    await userEvent.click(await screen.findByRole('button', { name: 'Remove' }));
    // First click only reveals the confirm step — nothing deleted yet.
    expect(fetchSpy.mock.calls.some(([, init]) => init?.method === 'DELETE')).toBe(false);
    await userEvent.click(screen.getByRole('button', { name: 'Remove repo' }));
    expect(fetchSpy.mock.calls.some(([, init]) => init?.method === 'DELETE')).toBe(true);
  });

  it('cancelling the detach keeps the repo attached', async () => {
    const ready = { paper_id: 'p1', repo_url: 'https://github.com/x/y', commit_sha: 'abc1234', status: 'ready', error: null, cloned_at: 'now', size_bytes: 1 };
    const fetchSpy = vi.fn(async () => codeResponse({ attached: true, code: ready }));
    vi.stubGlobal('fetch', fetchSpy);
    render(DockCode, { props: { id: 'p1' } });
    await userEvent.click(await screen.findByRole('button', { name: 'Remove' }));
    await userEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByRole('button', { name: 'Remove repo' })).not.toBeInTheDocument();
    expect(screen.getByText(/abc1234/)).toBeInTheDocument();
  });

  it('shows the error state with the message', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => codeResponse({ attached: true, code: { paper_id: 'p1', repo_url: 'https://x/y', commit_sha: null, status: 'error', error: 'git clone failed', cloned_at: null, size_bytes: null } })));
    render(DockCode, { props: { id: 'p1' } });
    expect(await screen.findByText(/git clone failed/)).toBeInTheDocument();
  });
});
