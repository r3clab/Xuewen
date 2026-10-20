import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it } from 'vitest';
import PdfQuickActions from './PdfQuickActions.svelte';
import { chat } from '$features/reader/ask/chat.svelte';
import { viewer } from '$features/reader/tabs/state.svelte';
import { dock } from '$features/reader/dock/state.svelte';
import type { PillHide } from './pillHide.svelte';

/// A plain fake matching the real `PillHide` interface (see
/// `pdf/pillHide.svelte.ts`) — `createPillHide` registers `$effect`s and is
/// meant to be constructed during a component's own init, so tests build the
/// shape directly instead of calling the factory standalone.
function makePill(): PillHide {
  return {
    visible: true,
    toolbarVisible: true,
    setHost() {},
    setExtraHold() {},
    onWindowMove() {},
    onScroll() {},
    onScrollJump() {},
    pillEnter() {},
    pillLeave() {},
    focusIn() {},
    focusOut() {},
  };
}

beforeEach(() => {
  viewer.activeId = 'p1';
  dock.open = false;
  dock.entry = null;
  viewer.zen = false;
  chat.available = true;
  localStorage.clear();
});

describe('PdfQuickActions seals', () => {
  it('renders one seal — 問 — and no zen or translate toggle', () => {
    render(PdfQuickActions, { props: { pill: makePill() } });
    expect(screen.getByRole('button', { name: 'Paper panel' })).toHaveTextContent('問');
    expect(screen.getAllByRole('button')).toHaveLength(1);
    expect(screen.queryByRole('button', { name: 'Zen mode' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /translate/i })).not.toBeInTheDocument();
  });

  it('falls back to 詳 when chat is unavailable — the panel is then the record alone', () => {
    chat.available = false;
    render(PdfQuickActions, { props: { pill: makePill() } });
    expect(screen.getByRole('button', { name: 'Paper panel' })).toHaveTextContent('詳');
  });

  it('the seal opens the dock on the composer, or on the record without chat', async () => {
    render(PdfQuickActions, { props: { pill: makePill() } });
    await userEvent.click(screen.getByRole('button', { name: 'Paper panel' }));
    expect(dock.open).toBe(true);
    expect(dock.entry).toBe('ask');

    dock.open = false;
    dock.entry = null;
    chat.available = false;
    render(PdfQuickActions, { props: { pill: makePill() } });
    await userEvent.click(screen.getAllByRole('button', { name: 'Paper panel' })[1]);
    expect(dock.open).toBe(true);
    expect(dock.entry).toBe('record');
  });

  it('the pill yields while the dock is open', () => {
    dock.open = true;
    render(PdfQuickActions, { props: { pill: makePill() } });
    expect(screen.getByRole('toolbar', { name: 'Reader quick actions' }).className).toContain('opacity-0');
  });
});
