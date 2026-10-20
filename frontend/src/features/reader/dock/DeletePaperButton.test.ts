import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import DeletePaperButton from './DeletePaperButton.svelte';
import { library } from '$features/library/state.svelte';
import { viewer } from '$features/reader/tabs/state.svelte';

beforeEach(() => {
  library.papers = [];
  viewer.tabs = [];
  viewer.activeId = null;
});

describe('DeletePaperButton', () => {
  it('requires confirmation before deleting', async () => {
    const fetchMock = vi.fn(async () =>
      new Response('{}', { status: 200, headers: { 'content-type': 'application/json' } }),
    );
    vi.stubGlobal('fetch', fetchMock);

    render(DeletePaperButton, { props: { id: 'p1' } });
    await userEvent.click(screen.getByRole('button', { name: /Delete paper/ }));
    expect(fetchMock).not.toHaveBeenCalled(); // confirm step first

    await userEvent.click(screen.getByRole('button', { name: 'Delete' }));
    expect(fetchMock).toHaveBeenCalled();
    expect((globalThis.fetch as ReturnType<typeof vi.fn>).mock.calls[0][1]).toMatchObject({
      method: 'DELETE',
    });
  });

  it('cancels back to the plain button without deleting', async () => {
    const fetchMock = vi.fn();
    vi.stubGlobal('fetch', fetchMock);

    render(DeletePaperButton, { props: { id: 'p1' } });
    await userEvent.click(screen.getByRole('button', { name: /Delete paper/ }));
    await userEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    expect(fetchMock).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: /Delete paper/ })).toBeInTheDocument();
  });
});
