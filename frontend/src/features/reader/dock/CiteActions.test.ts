import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import CiteActions from './CiteActions.svelte';
import { toasts } from '$shared/ui/toasts.svelte';

beforeEach(() => {
  toasts.items.length = 0;
  vi.unstubAllGlobals();
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => new Response('@article{key2024}', { status: 200 })),
  );
});

describe('CiteActions', () => {
  it('copies the citation and confirms with a toast', async () => {
    const writeText = vi.fn(async () => {});
    vi.stubGlobal('navigator', { clipboard: { writeText } });
    render(CiteActions, { props: { id: 'p1', citeKey: 'key2024' } });
    await userEvent.click(screen.getByRole('button', { name: /copy/i }));
    expect(writeText).toHaveBeenCalledWith('@article{key2024}');
    expect(toasts.items.some((t) => t.kind === 'success')).toBe(true);
  });

  it('keeps the failure hint inline when copy is impossible', async () => {
    vi.stubGlobal('navigator', {}); // no clipboard API
    vi.stubGlobal('document', Object.assign(document, {})); // keep jsdom document
    // jsdom's execCommand is undefined -> the legacy path throws.
    render(CiteActions, { props: { id: 'p1', citeKey: 'key2024' } });
    await userEvent.click(screen.getByRole('button', { name: /copy/i }));
    expect(screen.getByText(/use Download instead/i)).toBeInTheDocument();
  });
});
