import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { createRawSnippet } from 'svelte';
import Modal from './Modal.svelte';

function body() {
  return createRawSnippet(() => ({ render: () => '<p>modal body</p>' }));
}

describe('Modal', () => {
  it('renders a dialog with the title and body', () => {
    render(Modal, { props: { title: 'Test dialog', onclose: () => {}, children: body() } });
    expect(screen.getByRole('dialog', { name: 'Test dialog' })).toBeInTheDocument();
    expect(screen.getByText('modal body')).toBeInTheDocument();
  });

  it('closes on Escape', async () => {
    const onclose = vi.fn();
    render(Modal, { props: { title: 'Test dialog', onclose, children: body() } });
    await userEvent.keyboard('{Escape}');
    expect(onclose).toHaveBeenCalled();
  });

  it('renders no footer region when footer is omitted', () => {
    render(Modal, { props: { title: 'Test dialog', onclose: () => {}, children: body() } });
    const dialog = screen.getByRole('dialog', { name: 'Test dialog' });
    expect(dialog.lastElementChild?.className).not.toContain('border-t');
  });

  it('closes on backdrop click but not on panel click', async () => {
    const onclose = vi.fn();
    render(Modal, { props: { title: 'Test dialog', onclose, children: body() } });
    await userEvent.click(screen.getByText('modal body'));
    expect(onclose).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole('presentation'));
    expect(onclose).toHaveBeenCalledTimes(1);
  });
});
