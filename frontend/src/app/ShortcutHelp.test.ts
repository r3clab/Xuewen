import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it } from 'vitest';
import ShortcutHelp from './ShortcutHelp.svelte';
import { shell } from './shell.svelte';

beforeEach(() => {
  shell.helpOpen = true;
});

describe('ShortcutHelp', () => {
  it('lists the app shortcuts', () => {
    render(ShortcutHelp);
    expect(screen.getByText('Search library')).toBeInTheDocument();
    expect(screen.getByText('Zen mode')).toBeInTheDocument();
    expect(screen.getByText('Find paper…')).toBeInTheDocument();
    expect(screen.getByText('Next / previous paper')).toBeInTheDocument();
    expect(screen.getByText('Find in PDF')).toBeInTheDocument();
    expect(screen.getByText('Copy selected text')).toBeInTheDocument();
  });

  it('closes on Escape', async () => {
    render(ShortcutHelp);
    await userEvent.keyboard('{Escape}');
    expect(shell.helpOpen).toBe(false);
  });
});
