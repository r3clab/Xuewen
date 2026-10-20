import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import TranslateBubble from './TranslateBubble.svelte';

describe('TranslateBubble', () => {
  it('renders at a point and fires onTranslate when clicked', async () => {
    const onTranslate = vi.fn();
    render(TranslateBubble, { props: { x: 50, y: 60, onTranslate } });
    await userEvent.click(screen.getByRole('button', { name: /translate/i }));
    expect(onTranslate).toHaveBeenCalled();
  });
});
