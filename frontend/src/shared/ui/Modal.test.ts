import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createRawSnippet } from 'svelte';
import Modal from './Modal.svelte';
import { dismissTopLayer, modalOpen } from './escape';

function body() {
  return createRawSnippet(() => ({ render: () => '<p>modal body</p>' }));
}

beforeEach(() => window.addEventListener('keydown', dismissTopLayer));
afterEach(() => window.removeEventListener('keydown', dismissTopLayer));

describe('Modal', () => {
  it('renders a modal dialog with the title and body', () => {
    render(Modal, { props: { title: 'Test dialog', onclose: () => {}, children: body() } });
    const dialog = screen.getByRole('dialog', { name: 'Test dialog' });
    expect(dialog).toHaveProperty('open', true);
    expect(screen.getByText('modal body')).toBeInTheDocument();
    expect(modalOpen()).toBe(true);
  });

  it('closes on Escape through the layer stack', async () => {
    const onclose = vi.fn();
    render(Modal, { props: { title: 'Test dialog', onclose, children: body() } });
    await userEvent.keyboard('{Escape}');
    expect(onclose).toHaveBeenCalledTimes(1);
  });

  it('closes on the browser cancel event too', () => {
    const onclose = vi.fn();
    render(Modal, { props: { title: 'Test dialog', onclose, children: body() } });
    const cancel = new Event('cancel', { cancelable: true });
    screen.getByRole('dialog').dispatchEvent(cancel);
    expect(onclose).toHaveBeenCalledTimes(1);
    expect(cancel.defaultPrevented).toBe(true);
  });

  it('moves focus in on open and back to the opener on close', () => {
    const opener = document.createElement('button');
    document.body.append(opener);
    opener.focus();
    const { unmount } = render(Modal, { props: { title: 'Test dialog', onclose: () => {}, children: body() } });
    expect(screen.getByRole('dialog').contains(document.activeElement)).toBe(true);
    unmount();
    expect(document.activeElement).toBe(opener);
    expect(modalOpen()).toBe(false);
    opener.remove();
  });

  it('renders no footer region when footer is omitted', () => {
    render(Modal, { props: { title: 'Test dialog', onclose: () => {}, children: body() } });
    expect(screen.getByRole('dialog').querySelector('.border-t')).toBeNull();
  });

  it('closes on backdrop click but not on panel click', async () => {
    const onclose = vi.fn();
    render(Modal, { props: { title: 'Test dialog', onclose, children: body() } });
    await userEvent.click(screen.getByText('modal body'));
    expect(onclose).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole('dialog'));
    expect(onclose).toHaveBeenCalledTimes(1);
  });
});
