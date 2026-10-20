import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import Toaster from './Toaster.svelte';
import { toast, toasts } from './toasts.svelte';

beforeEach(() => {
  toasts.items = [];
});

describe('Toaster', () => {
  it('announces error toasts assertively via role=alert', () => {
    toast('error', 'Delete failed', 0);
    render(Toaster);
    expect(screen.getByRole('alert')).toHaveTextContent('Delete failed');
  });

  it('renders the action button; clicking runs it and dismisses the toast', async () => {
    const run = vi.fn();
    toast('success', 'Paper deleted', 0, { label: 'Undo', run });
    render(Toaster);
    await userEvent.click(screen.getByRole('button', { name: 'Undo' }));
    expect(run).toHaveBeenCalledOnce();
    expect(toasts.items).toHaveLength(0);
  });

  it('keeps non-error toasts polite (no alert role)', () => {
    toast('success', 'Saved', 0);
    render(Toaster);
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    expect(screen.getByText('Saved')).toBeInTheDocument();
  });
});
