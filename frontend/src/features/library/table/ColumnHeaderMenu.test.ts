import { render, screen, waitFor } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import ColumnHeaderMenu from './ColumnHeaderMenu.svelte';

function renderMenu() {
  const onAutoFitAll = vi.fn();
  const onReset = vi.fn();
  const onClose = vi.fn();
  const utils = render(ColumnHeaderMenu, {
    props: { x: 10, y: 10, onAutoFitAll, onReset, onClose },
  });
  return { onAutoFitAll, onReset, onClose, ...utils };
}

describe('ColumnHeaderMenu', () => {
  it('renders both actions and focuses the first on open', async () => {
    renderMenu();
    expect(screen.getByRole('menu', { name: 'Column options' })).toBeInTheDocument();
    await waitFor(() =>
      expect(screen.getByRole('menuitem', { name: /auto-fit all columns/i })).toHaveFocus(),
    );
    expect(screen.getByRole('menuitem', { name: /reset to default widths/i })).toBeInTheDocument();
  });

  it('Auto-fit all runs the action, then closes', async () => {
    const { onAutoFitAll, onClose } = renderMenu();
    await userEvent.click(screen.getByRole('menuitem', { name: /auto-fit all columns/i }));
    expect(onAutoFitAll).toHaveBeenCalledOnce();
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('Reset runs the action, then closes', async () => {
    const { onReset, onClose } = renderMenu();
    await userEvent.click(screen.getByRole('menuitem', { name: /reset to default widths/i }));
    expect(onReset).toHaveBeenCalledOnce();
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('Escape closes the menu', async () => {
    const { onClose } = renderMenu();
    await userEvent.keyboard('{Escape}');
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('a non-bubbling inner-pane scroll dismisses (capture-phase listener)', async () => {
    const { onClose } = renderMenu();
    await waitFor(() =>
      expect(screen.getByRole('menuitem', { name: /auto-fit all columns/i })).toHaveFocus(),
    );
    // The table pane's scroll event does not bubble to window — only a
    // capture-phase listener hears it. Dismissing here is what keeps the
    // menu from floating detached from its header when the list scrolls.
    const pane = document.createElement('div');
    document.body.appendChild(pane);
    pane.dispatchEvent(new Event('scroll'));
    expect(onClose).toHaveBeenCalledOnce();
    pane.remove();
  });

  it('restores focus to the previously focused element on close', async () => {
    const outside = document.createElement('button');
    document.body.appendChild(outside);
    outside.focus();
    const { unmount } = renderMenu();
    await waitFor(() =>
      expect(screen.getByRole('menuitem', { name: /auto-fit all columns/i })).toHaveFocus(),
    );
    unmount();
    expect(outside).toHaveFocus();
    outside.remove();
  });
});
